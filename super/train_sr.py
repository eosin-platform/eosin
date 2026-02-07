#!/usr/bin/env python3
"""
Pathology Super-Resolution Training Script

Trains a generator G that upsamples 40x histology tiles (128x128) to 4x super-resolution
(512x512), hallucinating higher-than-optical resolution details.

This is an UNSUPERVISED approach - we have no ground-truth at the target resolution.
Training uses:
  - Cycle consistency: downsampling the SR image should reconstruct the original
  - Adversarial loss: discriminator distinguishes real vs generated high-res patches
  - Perceptual/edge losses: encourage realistic textures and sharp edges
"""

import argparse
import csv
import io
import os
import random
import time
from dataclasses import dataclass
from typing import List, Optional, Tuple

import grpc
import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from PIL import Image
from torch.cuda.amp import GradScaler, autocast
from torch.utils.data import DataLoader, Dataset
from torch.utils.tensorboard import SummaryWriter
from torchvision import transforms
from torchvision.utils import make_grid, save_image

try:
    import openslide
    OPENSLIDE_AVAILABLE = True
except ImportError:
    OPENSLIDE_AVAILABLE = False

try:
    from torchvision.models import vgg19, VGG19_Weights
    VGG_AVAILABLE = True
except ImportError:
    VGG_AVAILABLE = False

# Assume these are generated and importable
import storage_pb2
import storage_pb2_grpc


# =============================================================================
# Degradation Model (Synthetic LR Generation)
# =============================================================================

class DegradationModel:
    """
    Realistic degradation pipeline for creating synthetic low-resolution images.
    Simulates optical blur, sensor noise, and compression artifacts.
    """
    
    def __init__(
        self,
        scale_factor: float = 0.5,
        blur_kernel_size: int = 5,
        blur_sigma_range: Tuple[float, float] = (0.5, 1.5),
        noise_sigma_range: Tuple[float, float] = (0.0, 0.03),
        jpeg_quality_range: Tuple[int, int] = (70, 95),
        apply_blur_prob: float = 0.7,
        apply_noise_prob: float = 0.5,
        apply_jpeg_prob: float = 0.3,
    ):
        self.scale_factor = scale_factor
        self.blur_kernel_size = blur_kernel_size
        self.blur_sigma_range = blur_sigma_range
        self.noise_sigma_range = noise_sigma_range
        self.jpeg_quality_range = jpeg_quality_range
        self.apply_blur_prob = apply_blur_prob
        self.apply_noise_prob = apply_noise_prob
        self.apply_jpeg_prob = apply_jpeg_prob
    
    def _gaussian_blur(self, img: Image.Image) -> Image.Image:
        """Apply Gaussian blur to simulate optical aberration."""
        from PIL import ImageFilter
        sigma = random.uniform(*self.blur_sigma_range)
        return img.filter(ImageFilter.GaussianBlur(radius=sigma))
    
    def _add_noise(self, tensor: torch.Tensor) -> torch.Tensor:
        """Add Gaussian noise to simulate sensor noise."""
        sigma = random.uniform(*self.noise_sigma_range)
        noise = torch.randn_like(tensor) * sigma
        return torch.clamp(tensor + noise, 0, 1)
    
    def _jpeg_compress(self, img: Image.Image) -> Image.Image:
        """Apply JPEG compression artifacts."""
        quality = random.randint(*self.jpeg_quality_range)
        buffer = io.BytesIO()
        img.save(buffer, format='JPEG', quality=quality)
        buffer.seek(0)
        return Image.open(buffer).convert('RGB')
    
    def degrade_pil(self, hr_img: Image.Image) -> Image.Image:
        """Apply degradation pipeline to PIL image."""
        img = hr_img.copy()
        
        # Apply blur before downsampling (simulates optical blur)
        if random.random() < self.apply_blur_prob:
            img = self._gaussian_blur(img)
        
        # Downsample
        new_size = (int(img.width * self.scale_factor), int(img.height * self.scale_factor))
        try:
            resample = Image.Resampling.BICUBIC
        except AttributeError:
            resample = Image.BICUBIC  # type: ignore
        img = img.resize(new_size, resample)
        
        # Apply JPEG compression (common in medical imaging pipelines)
        if random.random() < self.apply_jpeg_prob:
            img = self._jpeg_compress(img)
        
        return img
    
    def degrade_tensor(self, hr_tensor: torch.Tensor) -> torch.Tensor:
        """Apply degradation to tensor, returns LR tensor."""
        # hr_tensor: (C, H, W) in [0, 1]
        lr = F.interpolate(
            hr_tensor.unsqueeze(0), 
            scale_factor=self.scale_factor, 
            mode='bicubic', 
            align_corners=False
        ).squeeze(0)
        lr = torch.clamp(lr, 0, 1)
        
        # Add noise
        if random.random() < self.apply_noise_prob:
            lr = self._add_noise(lr)
        
        return lr


# =============================================================================
# Perceptual and Sharpness Losses
# =============================================================================

class VGGPerceptualLoss(nn.Module):
    """VGG-based perceptual loss for texture and structure preservation."""
    
    def __init__(self, layer_weights: Optional[dict] = None):
        super().__init__()
        if not VGG_AVAILABLE:
            raise ImportError("torchvision with VGG19 required for perceptual loss")
        
        vgg = vgg19(weights=VGG19_Weights.IMAGENET1K_V1).features.eval()
        
        # Extract features at multiple scales
        # relu1_2, relu2_2, relu3_4, relu4_4, relu5_4
        self.slice1 = nn.Sequential(*list(vgg[:4]))
        self.slice2 = nn.Sequential(*list(vgg[4:9]))
        self.slice3 = nn.Sequential(*list(vgg[9:18]))
        self.slice4 = nn.Sequential(*list(vgg[18:27]))
        self.slice5 = nn.Sequential(*list(vgg[27:36]))
        
        # Freeze all parameters
        for param in self.parameters():
            param.requires_grad = False
        
        # Default weights emphasizing mid-level features (edges, textures)
        self.weights = layer_weights or {
            'relu1_2': 0.1,
            'relu2_2': 0.2,
            'relu3_4': 0.4,
            'relu4_4': 0.2,
            'relu5_4': 0.1,
        }
        
        # ImageNet normalization
        self.register_buffer('mean', torch.tensor([0.485, 0.456, 0.406]).view(1, 3, 1, 1))
        self.register_buffer('std', torch.tensor([0.229, 0.224, 0.225]).view(1, 3, 1, 1))
    
    def _normalize(self, x: torch.Tensor) -> torch.Tensor:
        return (x - self.mean) / self.std
    
    def forward(self, pred: torch.Tensor, target: torch.Tensor) -> torch.Tensor:
        pred = self._normalize(pred)
        target = self._normalize(target)
        
        loss = 0.0
        
        # Extract features
        p1, t1 = self.slice1(pred), self.slice1(target)
        loss += self.weights['relu1_2'] * F.l1_loss(p1, t1)
        
        p2, t2 = self.slice2(p1), self.slice2(t1)
        loss += self.weights['relu2_2'] * F.l1_loss(p2, t2)
        
        p3, t3 = self.slice3(p2), self.slice3(t2)
        loss += self.weights['relu3_4'] * F.l1_loss(p3, t3)
        
        p4, t4 = self.slice4(p3), self.slice4(t3)
        loss += self.weights['relu4_4'] * F.l1_loss(p4, t4)
        
        p5, t5 = self.slice5(p4), self.slice5(t4)
        loss += self.weights['relu5_4'] * F.l1_loss(p5, t5)
        
        return loss


class EdgeLoss(nn.Module):
    """Edge-aware loss using Sobel gradients for morphological clarity."""
    
    def __init__(self):
        super().__init__()
        # Sobel kernels
        sobel_x = torch.tensor([[-1, 0, 1], [-2, 0, 2], [-1, 0, 1]], dtype=torch.float32)
        sobel_y = torch.tensor([[-1, -2, -1], [0, 0, 0], [1, 2, 1]], dtype=torch.float32)
        
        # Expand for 3 channels
        self.register_buffer('sobel_x', sobel_x.view(1, 1, 3, 3).repeat(3, 1, 1, 1))
        self.register_buffer('sobel_y', sobel_y.view(1, 1, 3, 3).repeat(3, 1, 1, 1))
    
    def _gradient_magnitude(self, x: torch.Tensor) -> torch.Tensor:
        # Convert to grayscale for edge detection
        gray = 0.299 * x[:, 0:1] + 0.587 * x[:, 1:2] + 0.114 * x[:, 2:3]
        
        gx = F.conv2d(gray, self.sobel_x[:1], padding=1)
        gy = F.conv2d(gray, self.sobel_y[:1], padding=1)
        
        return torch.sqrt(gx ** 2 + gy ** 2 + 1e-8)
    
    def forward(self, pred: torch.Tensor, target: torch.Tensor) -> torch.Tensor:
        pred_edges = self._gradient_magnitude(pred)
        target_edges = self._gradient_magnitude(target)
        
        # L1 loss on edge magnitudes
        return F.l1_loss(pred_edges, target_edges)


class FrequencyLoss(nn.Module):
    """FFT-based frequency loss to encourage high-frequency detail generation."""
    
    def __init__(self, focus_high_freq: bool = True, high_freq_weight: float = 2.0):
        super().__init__()
        self.focus_high_freq = focus_high_freq
        self.high_freq_weight = high_freq_weight
    
    def _get_frequency_weights(self, h: int, w: int, device: torch.device) -> torch.Tensor:
        """Create frequency-dependent weights (higher weight for high frequencies)."""
        cy, cx = h // 2, w // 2
        y = torch.arange(h, device=device).float() - cy
        x = torch.arange(w, device=device).float() - cx
        yy, xx = torch.meshgrid(y, x, indexing='ij')
        
        # Distance from center (DC component)
        dist = torch.sqrt(xx ** 2 + yy ** 2)
        max_dist = torch.sqrt(torch.tensor(cy ** 2 + cx ** 2, dtype=torch.float32))
        
        # Weight increases with frequency
        weights = 1.0 + (self.high_freq_weight - 1.0) * (dist / max_dist)
        return weights
    
    def forward(self, pred: torch.Tensor, target: torch.Tensor) -> torch.Tensor:
        # Convert to grayscale
        pred_gray = 0.299 * pred[:, 0] + 0.587 * pred[:, 1] + 0.114 * pred[:, 2]
        target_gray = 0.299 * target[:, 0] + 0.587 * target[:, 1] + 0.114 * target[:, 2]
        
        # FFT
        pred_fft = torch.fft.fft2(pred_gray)
        target_fft = torch.fft.fft2(target_gray)
        
        # Shift DC to center
        pred_fft = torch.fft.fftshift(pred_fft)
        target_fft = torch.fft.fftshift(target_fft)
        
        # Magnitude spectrum
        pred_mag = torch.abs(pred_fft)
        target_mag = torch.abs(target_fft)
        
        # Apply frequency weights if focusing on high frequencies
        if self.focus_high_freq:
            weights = self._get_frequency_weights(
                pred_mag.shape[-2], pred_mag.shape[-1], pred.device
            )
            diff = (pred_mag - target_mag) * weights
        else:
            diff = pred_mag - target_mag
        
        # Log-scale for numerical stability
        return torch.mean(torch.abs(diff) / (target_mag + 1e-8))


class LaplacianLoss(nn.Module):
    """Laplacian-based sharpness loss for crisp edges."""
    
    def __init__(self):
        super().__init__()
        # Laplacian kernel
        laplacian = torch.tensor(
            [[0, 1, 0], [1, -4, 1], [0, 1, 0]], 
            dtype=torch.float32
        )
        self.register_buffer('laplacian', laplacian.view(1, 1, 3, 3))
    
    def _compute_laplacian(self, x: torch.Tensor) -> torch.Tensor:
        gray = 0.299 * x[:, 0:1] + 0.587 * x[:, 1:2] + 0.114 * x[:, 2:3]
        return F.conv2d(gray, self.laplacian, padding=1)
    
    def forward(self, pred: torch.Tensor, target: torch.Tensor) -> torch.Tensor:
        pred_lap = self._compute_laplacian(pred)
        target_lap = self._compute_laplacian(target)
        return F.l1_loss(pred_lap, target_lap)


# =============================================================================
# Data Structures
# =============================================================================

@dataclass
class SlideMetadata:
    """Metadata for a single slide."""
    slide_id: str  # UUID hex string
    filename: str  # TIF filename (without .tif extension)
    width_px: int
    height_px: int
    max_tiles_x: int  # width_px // 128
    max_tiles_y: int  # height_px // 128


def load_csv(csv_path: str) -> List[SlideMetadata]:
    """Load slide metadata from CSV file.
    
    Expected format: slide_id,filename,width_px,height_px
    The filename should be the TIF filename (with or without .tif extension).
    """
    slides = []
    with open(csv_path, 'r', newline='') as f:
        reader = csv.reader(f)
        for row in reader:
            if len(row) < 4:
                continue
            slide_id = row[0].strip()
            # Strip .tif extension if present for consistency
            filename = row[1].strip()
            if filename.lower().endswith('.tif'):
                filename = filename[:-4]
            width_px = int(row[2])
            height_px = int(row[3])
            max_tiles_x = width_px // 128
            max_tiles_y = height_px // 128
            # Only include slides with at least one full tile
            if max_tiles_x > 0 and max_tiles_y > 0:
                slides.append(SlideMetadata(
                    slide_id=slide_id,
                    filename=filename,
                    width_px=width_px,
                    height_px=height_px,
                    max_tiles_x=max_tiles_x,
                    max_tiles_y=max_tiles_y,
                ))
    return slides


def uuid_hex_to_bytes(uuid_hex: str) -> bytes:
    """Convert UUID hex string to 16 raw bytes."""
    # Remove dashes and convert to bytes
    hex_clean = uuid_hex.replace('-', '')
    return bytes.fromhex(hex_clean)


def has_content(img: Image.Image, min_std: float = 5.0, min_mean: float = 10.0) -> bool:
    """
    Check if an image tile has meaningful content.
    
    TIF files are sparse, so many tiles may be completely black or near-black.
    This function filters out empty tiles by checking pixel statistics.
    
    Args:
        img: PIL Image to check
        min_std: Minimum standard deviation threshold (indicates variation)
        min_mean: Minimum mean pixel value threshold (filters pure black)
    
    Returns:
        True if the tile has meaningful content, False otherwise
    """
    arr = np.array(img, dtype=np.float32)
    # Check if image has enough variation (not uniform)
    if arr.std() < min_std:
        return False
    # Check if image is not predominantly black
    if arr.mean() < min_mean:
        return False
    return True


def check_grpc_health(grpc_address: str, timeout: float = 10.0) -> bool:
    """Test gRPC connection with a health check."""
    print(f"Testing gRPC connection to {grpc_address}...")
    try:
        channel = grpc.insecure_channel(grpc_address)
        stub = storage_pb2_grpc.StorageApiStub(channel)
        
        # Try health check RPC
        request = storage_pb2.HealthCheckRequest()
        response = stub.HealthCheck(request, timeout=timeout)
        print(f"Health check passed: healthy={response.healthy}")
        channel.close()
        return response.healthy
    except grpc.RpcError as e:
        print(f"Health check failed: {e.code()} - {e.details()}")
        channel.close()
        return False
    except Exception as e:
        print(f"Health check error: {e}")
        return False


# =============================================================================
# Dataset
# =============================================================================

class OpenSlideTileDataset(Dataset):
    """
    PyTorch Dataset that extracts tiles from local TIF files using OpenSlide.
    Returns full-resolution tiles for unsupervised super-resolution training.
    """

    def __init__(
        self,
        slides: List[SlideMetadata],
        data_root: str,
        train_level: int = 0,
        target_tile_size: int = 128,
        tiles_per_slide: int = 100,
        color_jitter: bool = False,
        color_jitter_strength: float = 0.05,
    ):
        if not OPENSLIDE_AVAILABLE:
            raise ImportError("openslide-python is required for --data-root. Install with: pip install openslide-python")

        self.slides = slides
        self.data_root = data_root
        self.train_level = train_level
        self.target_tile_size = target_tile_size
        self.tiles_per_slide = tiles_per_slide
        self.color_jitter = color_jitter

        # Cache for OpenSlide objects (opened lazily)
        self._slide_cache: dict = {}

        # Augmentation transforms
        self.to_tensor = transforms.ToTensor()
        if color_jitter:
            self.jitter = transforms.ColorJitter(
                brightness=color_jitter_strength,
                contrast=color_jitter_strength,
                saturation=color_jitter_strength,
                hue=color_jitter_strength * 0.5,
            )
        else:
            self.jitter = None

    def _get_slide(self, filename: str) -> openslide.OpenSlide:
        """Get or open an OpenSlide object for the given slide."""
        if filename not in self._slide_cache:
            tif_path = os.path.join(self.data_root, f"{filename}.tif")
            if not os.path.exists(tif_path):
                raise FileNotFoundError(f"TIF file not found: {tif_path}")
            self._slide_cache[filename] = openslide.OpenSlide(tif_path)
        return self._slide_cache[filename]

    def __len__(self) -> int:
        return len(self.slides) * self.tiles_per_slide

    def _get_random_tile_coords(self, slide: SlideMetadata) -> Tuple[int, int]:
        """Get random valid tile coordinates for a slide."""
        tile_x = random.randint(0, slide.max_tiles_x - 1)
        tile_y = random.randint(0, slide.max_tiles_y - 1)
        return tile_x, tile_y

    def _extract_tile(self, slide: SlideMetadata, tile_x: int, tile_y: int) -> Optional[Image.Image]:
        """Extract a tile from the TIF file using OpenSlide."""
        try:
            osr = self._get_slide(slide.filename)
            
            # Calculate pixel coordinates from tile indices
            # At the training level, each tile is 128x128 pixels
            # OpenSlide read_region expects (x, y) in level 0 coordinates
            level = self.train_level
            
            # Get the downsample factor for this level
            if level >= osr.level_count:
                level = osr.level_count - 1
            downsample = osr.level_downsamples[level]
            
            # Tile coordinates in pixels at the training level
            px_x = tile_x * self.target_tile_size
            px_y = tile_y * self.target_tile_size
            
            # Convert to level 0 coordinates for read_region
            level0_x = int(px_x * downsample)
            level0_y = int(px_y * downsample)
            
            # Read the region at the specified level
            img = osr.read_region(
                (level0_x, level0_y),
                level,
                (self.target_tile_size, self.target_tile_size)
            )
            
            # Convert RGBA to RGB (OpenSlide returns RGBA)
            img = img.convert('RGB')
            return img
            
        except Exception as e:
            print(f"Error extracting tile [{slide.slide_id}, {tile_x}, {tile_y}]: {e}")
            return None

    def _apply_augmentations(self, img: Image.Image) -> Image.Image:
        """Apply spatial augmentations to PIL image."""
        # Handle Pillow version differences
        try:
            flip_h = Image.Transpose.FLIP_LEFT_RIGHT
            flip_v = Image.Transpose.FLIP_TOP_BOTTOM
        except AttributeError:
            flip_h = Image.FLIP_LEFT_RIGHT  # type: ignore
            flip_v = Image.FLIP_TOP_BOTTOM  # type: ignore
        
        # Random horizontal flip
        if random.random() > 0.5:
            img = img.transpose(flip_h)

        # Random vertical flip
        if random.random() > 0.5:
            img = img.transpose(flip_v)

        # Random 90° rotations
        k = random.randint(0, 3)
        if k > 0:
            img = img.rotate(k * 90, expand=False)

        # Color jitter (optional)
        if self.jitter is not None:
            img = self.jitter(img)

        return img

    def __getitem__(self, idx: int) -> torch.Tensor:
        """Get a full-resolution tile for unsupervised super-resolution training."""
        # Map index to slide (round-robin)
        slide_idx = idx % len(self.slides)
        slide = self.slides[slide_idx]

        # Try to fetch a valid tile (retry with different coords if needed)
        max_attempts = 10
        for _ in range(max_attempts):
            tile_x, tile_y = self._get_random_tile_coords(slide)
            img = self._extract_tile(slide, tile_x, tile_y)

            if img is None:
                continue

            # Check if tile is exactly 128x128
            if img.size != (self.target_tile_size, self.target_tile_size):
                continue

            # Check if tile has meaningful content (not empty/black)
            if not has_content(img):
                continue

            # Apply spatial augmentations
            img = self._apply_augmentations(img)
            
            # Convert to tensor
            tensor = self.to_tensor(img)
            
            return tensor

        # If we couldn't get a valid tile, try a different slide
        new_slide_idx = random.randint(0, len(self.slides) - 1)
        return self.__getitem__(new_slide_idx * self.tiles_per_slide)

    def __del__(self):
        """Close all cached OpenSlide objects."""
        for osr in self._slide_cache.values():
            osr.close()


class GrpcTileDataset(Dataset):
    """
    PyTorch Dataset that fetches tiles via gRPC from StorageApi.
    Returns full-resolution tiles for unsupervised super-resolution training.
    """

    def __init__(
        self,
        slides: List[SlideMetadata],
        grpc_address: str,
        train_level: int = 0,
        target_tile_size: int = 128,
        tiles_per_slide: int = 100,
        color_jitter: bool = False,
        color_jitter_strength: float = 0.05,
        timeout: float = 30.0,
        max_retries: int = 3,
    ):
        self.slides = slides
        self.train_level = train_level
        self.target_tile_size = target_tile_size
        self.tiles_per_slide = tiles_per_slide
        self.color_jitter = color_jitter
        self.timeout = timeout
        self.max_retries = max_retries

        # Create gRPC channel and stub
        self.channel = grpc.insecure_channel(grpc_address)
        self.stub = storage_pb2_grpc.StorageApiStub(self.channel)

        # Augmentation transforms
        self.to_tensor = transforms.ToTensor()
        if color_jitter:
            self.jitter = transforms.ColorJitter(
                brightness=color_jitter_strength,
                contrast=color_jitter_strength,
                saturation=color_jitter_strength,
                hue=color_jitter_strength * 0.5,
            )
        else:
            self.jitter = None

    def __len__(self) -> int:
        return len(self.slides) * self.tiles_per_slide

    def _get_random_tile_coords(self, slide: SlideMetadata) -> Tuple[int, int]:
        """Get random valid tile coordinates for a slide."""
        tile_x = random.randint(0, slide.max_tiles_x - 1)
        tile_y = random.randint(0, slide.max_tiles_y - 1)
        return tile_x, tile_y

    def _fetch_tile(self, slide: SlideMetadata, tile_x: int, tile_y: int) -> Optional[Image.Image]:
        """Fetch a tile from the gRPC service."""
        request = storage_pb2.GetTileRequest(
            id=uuid_hex_to_bytes(slide.slide_id),
            x=tile_x,
            y=tile_y,
            level=self.train_level,
        )

        for attempt in range(self.max_retries):
            try:
                response = self.stub.GetTile(request, timeout=self.timeout)
                # Decode image from bytes
                img = Image.open(io.BytesIO(response.data))
                img = img.convert('RGB')
                print(f"Fetched tile [{slide.slide_id}, {tile_x}, {tile_y}, level {self.train_level}] (attempt {attempt + 1})")
                return img
            except grpc.RpcError as e:
                if attempt == self.max_retries - 1:
                    print(f"Failed to fetch tile [{slide.slide_id}, {tile_x}, {tile_y}, {self.train_level}] after {self.max_retries} attempts: {e}")
                    return None
                time.sleep(0.1 * (attempt + 1))
            except Exception as e:
                print(f"Error decoding tile: {e}")
                return None

        return None

    def _apply_augmentations(self, img: Image.Image) -> Image.Image:
        """Apply spatial augmentations to PIL image."""
        # Handle Pillow version differences
        try:
            flip_h = Image.Transpose.FLIP_LEFT_RIGHT
            flip_v = Image.Transpose.FLIP_TOP_BOTTOM
        except AttributeError:
            flip_h = Image.FLIP_LEFT_RIGHT  # type: ignore
            flip_v = Image.FLIP_TOP_BOTTOM  # type: ignore
        
        # Random horizontal flip
        if random.random() > 0.5:
            img = img.transpose(flip_h)

        # Random vertical flip
        if random.random() > 0.5:
            img = img.transpose(flip_v)

        # Random 90° rotations
        k = random.randint(0, 3)
        if k > 0:
            img = img.rotate(k * 90, expand=False)

        # Color jitter (optional)
        if self.jitter is not None:
            img = self.jitter(img)

        return img

    def __getitem__(self, idx: int) -> torch.Tensor:
        """Get a full-resolution tile for unsupervised super-resolution training."""
        # Map index to slide (round-robin)
        slide_idx = idx % len(self.slides)
        slide = self.slides[slide_idx]

        # Try to fetch a valid tile (retry with different coords if needed)
        max_attempts = 10
        for _ in range(max_attempts):
            tile_x, tile_y = self._get_random_tile_coords(slide)
            img = self._fetch_tile(slide, tile_x, tile_y)

            if img is None:
                continue

            # Check if tile is exactly 128x128
            if img.size != (self.target_tile_size, self.target_tile_size):
                continue

            # Check if tile has meaningful content (not empty/black)
            if not has_content(img):
                continue

            # Apply spatial augmentations
            img = self._apply_augmentations(img)
            
            # Convert to tensor
            tensor = self.to_tensor(img)
            
            return tensor

        # If we couldn't get a valid tile, try a different slide
        new_slide_idx = random.randint(0, len(self.slides) - 1)
        return self.__getitem__(new_slide_idx * self.tiles_per_slide)


# =============================================================================
# Model Components
# =============================================================================

class ChannelAttention(nn.Module):
    """Squeeze-and-excitation channel attention."""
    
    def __init__(self, channels: int, reduction: int = 16):
        super().__init__()
        self.avg_pool = nn.AdaptiveAvgPool2d(1)
        self.max_pool = nn.AdaptiveMaxPool2d(1)
        self.fc = nn.Sequential(
            nn.Linear(channels, channels // reduction, bias=False),
            nn.ReLU(inplace=True),
            nn.Linear(channels // reduction, channels, bias=False),
        )
        self.sigmoid = nn.Sigmoid()
    
    def forward(self, x: torch.Tensor) -> torch.Tensor:
        b, c, _, _ = x.shape
        avg_out = self.fc(self.avg_pool(x).view(b, c))
        max_out = self.fc(self.max_pool(x).view(b, c))
        attn = self.sigmoid(avg_out + max_out).view(b, c, 1, 1)
        return x * attn


class SpatialAttention(nn.Module):
    """Spatial attention for focusing on morphological structures."""
    
    def __init__(self, kernel_size: int = 7):
        super().__init__()
        padding = kernel_size // 2
        self.conv = nn.Conv2d(2, 1, kernel_size, padding=padding, bias=False)
        self.sigmoid = nn.Sigmoid()
    
    def forward(self, x: torch.Tensor) -> torch.Tensor:
        avg_out = torch.mean(x, dim=1, keepdim=True)
        max_out, _ = torch.max(x, dim=1, keepdim=True)
        attn = self.sigmoid(self.conv(torch.cat([avg_out, max_out], dim=1)))
        return x * attn


class CBAM(nn.Module):
    """Convolutional Block Attention Module."""
    
    def __init__(self, channels: int, reduction: int = 16, kernel_size: int = 7):
        super().__init__()
        self.channel_attn = ChannelAttention(channels, reduction)
        self.spatial_attn = SpatialAttention(kernel_size)
    
    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = self.channel_attn(x)
        x = self.spatial_attn(x)
        return x


class ResidualBlock(nn.Module):
    """Residual block with two convolutions and optional attention."""

    def __init__(self, channels: int, use_attention: bool = False):
        super().__init__()
        self.conv1 = nn.Conv2d(channels, channels, 3, padding=1)
        self.bn1 = nn.BatchNorm2d(channels)
        self.conv2 = nn.Conv2d(channels, channels, 3, padding=1)
        self.bn2 = nn.BatchNorm2d(channels)
        self.attention = CBAM(channels) if use_attention else None

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        residual = x
        out = F.relu(self.bn1(self.conv1(x)), inplace=True)
        out = self.bn2(self.conv2(out))
        if self.attention is not None:
            out = self.attention(out)
        return out + residual


class ResidualDenseBlock(nn.Module):
    """Residual Dense Block for richer feature extraction."""
    
    def __init__(self, channels: int, growth_channels: int = 32, num_layers: int = 5):
        super().__init__()
        self.layers = nn.ModuleList()
        for i in range(num_layers):
            in_ch = channels + i * growth_channels
            self.layers.append(nn.Sequential(
                nn.Conv2d(in_ch, growth_channels, 3, padding=1),
                nn.LeakyReLU(0.2, inplace=True),
            ))
        self.fusion = nn.Conv2d(channels + num_layers * growth_channels, channels, 1)
        self.scale = 0.2
    
    def forward(self, x: torch.Tensor) -> torch.Tensor:
        features = [x]
        for layer in self.layers:
            out = layer(torch.cat(features, dim=1))
            features.append(out)
        fused = self.fusion(torch.cat(features, dim=1))
        return x + self.scale * fused


class RRDB(nn.Module):
    """Residual-in-Residual Dense Block (from ESRGAN)."""
    
    def __init__(self, channels: int, growth_channels: int = 32):
        super().__init__()
        self.rdb1 = ResidualDenseBlock(channels, growth_channels)
        self.rdb2 = ResidualDenseBlock(channels, growth_channels)
        self.rdb3 = ResidualDenseBlock(channels, growth_channels)
        self.scale = 0.2
    
    def forward(self, x: torch.Tensor) -> torch.Tensor:
        out = self.rdb1(x)
        out = self.rdb2(out)
        out = self.rdb3(out)
        return x + self.scale * out


class Generator(nn.Module):
    """
    Enhanced super-resolution generator with RRDB blocks and attention.
    
    Takes a 128x128 input and produces a 512x512 output (4x upscaling)
    with hallucinated high-frequency details for higher-than-optical resolution.
    """

    def __init__(
        self,
        in_channels: int = 3,
        base_channels: int = 64,
        num_residual_blocks: int = 16,
        use_rrdb: bool = True,
        growth_channels: int = 32,
        scale_factor: int = 4,
    ):
        super().__init__()
        self.use_rrdb = use_rrdb
        self.scale_factor = scale_factor

        # Initial feature extraction
        self.conv_in = nn.Conv2d(in_channels, base_channels, 3, padding=1)

        # Main body - RRDB or standard residual blocks
        if use_rrdb:
            self.body = nn.Sequential(
                *[RRDB(base_channels, growth_channels) for _ in range(num_residual_blocks)]
            )
        else:
            # Use attention on every 4th block
            self.body = nn.Sequential(
                *[ResidualBlock(base_channels, use_attention=(i % 4 == 3)) 
                  for i in range(num_residual_blocks)]
            )

        # Post-body conv
        self.conv_mid = nn.Conv2d(base_channels, base_channels, 3, padding=1)

        # Upsampling via PixelShuffle (4x = 2x + 2x)
        self.upsample1 = nn.Sequential(
            nn.Conv2d(base_channels, base_channels * 4, 3, padding=1),
            nn.LeakyReLU(0.2, inplace=True),
            nn.PixelShuffle(2),
        )
        
        self.upsample2 = nn.Sequential(
            nn.Conv2d(base_channels, base_channels * 4, 3, padding=1),
            nn.LeakyReLU(0.2, inplace=True),
            nn.PixelShuffle(2),
        )

        # High-frequency detail enhancement (after upsampling)
        self.detail_enhance = nn.Sequential(
            nn.Conv2d(base_channels, base_channels, 3, padding=1),
            nn.LeakyReLU(0.2, inplace=True),
            CBAM(base_channels),
            nn.Conv2d(base_channels, base_channels, 3, padding=1),
            nn.LeakyReLU(0.2, inplace=True),
        )

        # Final output
        self.conv_out = nn.Sequential(
            nn.Conv2d(base_channels, base_channels, 3, padding=1),
            nn.LeakyReLU(0.2, inplace=True),
            nn.Conv2d(base_channels, in_channels, 3, padding=1),
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        """
        Forward pass.
        
        Args:
            x: Input tensor of shape (B, 3, H, W) in [0, 1]
            
        Returns:
            y: Super-resolved output (B, 3, 4H, 4W)
        """
        # Bicubic upsample input to target resolution (baseline)
        x_up = F.interpolate(x, scale_factor=self.scale_factor, mode='bicubic', align_corners=False)
        x_up = torch.clamp(x_up, 0, 1)

        # Extract features
        feat = self.conv_in(x)
        trunk = feat

        # Main body
        feat = self.body(feat)
        feat = self.conv_mid(feat)
        feat = feat + trunk  # Global residual

        # Upsample features (4x = 2x + 2x)
        feat = self.upsample1(feat)
        feat = self.upsample2(feat)

        # Detail enhancement
        feat = self.detail_enhance(feat)

        # Generate output (direct prediction, not residual)
        out = self.conv_out(feat)

        # Residual learning from bicubic baseline
        y = x_up + out
        y = torch.clamp(y, 0, 1)

        return y


class Discriminator(nn.Module):
    """
    Patch-based discriminator for 512x512 images.
    
    Outputs a grid of real/fake scores to distinguish
    real high-res patches from generated ones.
    """

    def __init__(self, in_channels: int = 3, base_channels: int = 64):
        super().__init__()

        def conv_block(in_ch, out_ch, stride=2, bn=True):
            layers = [nn.Conv2d(in_ch, out_ch, 4, stride, 1, bias=not bn)]
            if bn:
                layers.append(nn.InstanceNorm2d(out_ch))  # InstanceNorm for stability
            layers.append(nn.LeakyReLU(0.2, inplace=True))
            return nn.Sequential(*layers)

        self.model = nn.Sequential(
            # Input: (B, 3, 512, 512)
            conv_block(in_channels, base_channels, stride=2, bn=False),  # -> 256
            conv_block(base_channels, base_channels * 2, stride=2),       # -> 128
            conv_block(base_channels * 2, base_channels * 4, stride=2),   # -> 64
            conv_block(base_channels * 4, base_channels * 8, stride=2),   # -> 32
            conv_block(base_channels * 8, base_channels * 8, stride=1),   # -> 32
            nn.Conv2d(base_channels * 8, 1, 4, 1, 1),                     # -> 31x31 patch
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        """
        Args:
            x: Input tensor of shape (B, 3, 512, 512)
            
        Returns:
            Patch-wise real/fake scores
        """
        return self.model(x)


# =============================================================================
# Loss Functions and Utilities
# =============================================================================

def downsample(y: torch.Tensor, scale_factor: float = 0.25) -> torch.Tensor:
    """Differentiable bicubic downsampling (4x by default)."""
    return F.interpolate(y, scale_factor=scale_factor, mode='bicubic', align_corners=False)


def upsample(x: torch.Tensor, scale_factor: float = 4.0) -> torch.Tensor:
    """Differentiable bicubic upsampling (4x by default)."""
    return F.interpolate(x, scale_factor=scale_factor, mode='bicubic', align_corners=False)


def reconstruction_loss(y: torch.Tensor, x: torch.Tensor) -> torch.Tensor:
    """L1 loss between downsampled SR and original (cycle consistency)."""
    y_down = downsample(y)  # 4x downsample
    return F.l1_loss(y_down, x)


def regularization_loss(r: torch.Tensor) -> torch.Tensor:
    """L1 regularization on residual to keep it subtle."""
    return torch.mean(torch.abs(r))


def discriminator_loss(d_real: torch.Tensor, d_fake: torch.Tensor) -> torch.Tensor:
    """Least-squares GAN discriminator loss."""
    loss_real = torch.mean((d_real - 1.0) ** 2)
    loss_fake = torch.mean(d_fake ** 2)
    return 0.5 * (loss_real + loss_fake)


def generator_adversarial_loss(d_fake: torch.Tensor) -> torch.Tensor:
    """Least-squares GAN generator loss."""
    return 0.5 * torch.mean((d_fake - 1.0) ** 2)


def save_sample_grid(
    x: torch.Tensor,
    y: torch.Tensor,
    step: int,
    out_dir: str,
    num_samples: int = 4,
) -> None:
    """Save a grid showing original, bicubic upsampled, and SR output with labels."""
    from PIL import ImageDraw, ImageFont

    x = x[:num_samples].cpu()
    y = y[:num_samples].cpu()

    # Bicubic upsample for comparison
    x_up = F.interpolate(x, scale_factor=2, mode='bicubic', align_corners=False)
    x_up = torch.clamp(x_up, 0, 1)

    # Resize original to match for visualization
    x_vis = F.interpolate(x, scale_factor=2, mode='nearest')

    # Stack horizontally: original | bicubic | SR
    grid = torch.cat([x_vis, x_up, y], dim=3)  # Concat along width

    # Convert to PIL for annotation
    grid_np = (grid * 255).clamp(0, 255).byte()
    # Stack samples vertically
    rows = []
    for i in range(num_samples):
        row_tensor = grid_np[i]  # (C, H, W)
        row_np = row_tensor.permute(1, 2, 0).numpy()  # (H, W, C)
        rows.append(row_np)
    
    # Combine all rows vertically
    combined = np.vstack(rows)
    img = Image.fromarray(combined, mode='RGB')
    
    # Draw labels
    draw = ImageDraw.Draw(img)
    
    # Try to load a font, fall back to default if unavailable
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 24)
    except (IOError, OSError):
        try:
            font = ImageFont.truetype("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", 24)
        except (IOError, OSError):
            font = ImageFont.load_default()
    
    # Image width for each panel (1024 pixels each)
    panel_width = x_vis.shape[3]
    row_height = x_vis.shape[2]
    labels = ["Original (nearest)", "Bicubic", "Super-Res"]
    
    # Draw labels on each row
    for row_idx in range(num_samples):
        y_offset = row_idx * row_height
        for col_idx, label in enumerate(labels):
            x_pos = col_idx * panel_width + 10
            y_pos = y_offset + 10
            # Draw text with outline for visibility
            outline_color = (0, 0, 0)
            text_color = (255, 255, 255)
            for dx, dy in [(-1, -1), (-1, 1), (1, -1), (1, 1), (-2, 0), (2, 0), (0, -2), (0, 2)]:
                draw.text((x_pos + dx, y_pos + dy), label, font=font, fill=outline_color)
            draw.text((x_pos, y_pos), label, font=font, fill=text_color)

    # Save
    save_path = os.path.join(out_dir, f'sample_step_{step:06d}.png')
    img.save(save_path)
    print(f"Saved sample to {save_path}")


def log_vram_usage(max_vram_gb: float) -> None:
    """Log current VRAM usage and warn if exceeding budget."""
    if not torch.cuda.is_available():
        return

    allocated = torch.cuda.memory_allocated() / 1e9
    reserved = torch.cuda.memory_reserved() / 1e9

    if allocated > max_vram_gb:
        print(f"WARNING: VRAM usage ({allocated:.2f} GB) exceeds budget ({max_vram_gb} GB)")


def save_sample_grid_v2(
    lr: torch.Tensor,
    hr: torch.Tensor,
    sr: torch.Tensor,
    step: int,
    out_dir: str,
    num_samples: int = 4,
) -> None:
    """Save a grid showing LR, Bicubic, SR, and HR ground truth with labels."""
    from PIL import ImageDraw, ImageFont

    lr = lr[:num_samples].cpu()
    hr = hr[:num_samples].cpu()
    sr = sr[:num_samples].cpu()
    num_samples = lr.shape[0]  # Actual batch size after slicing

    # Bicubic upsample LR for comparison
    bicubic = F.interpolate(lr, scale_factor=2, mode='bicubic', align_corners=False)
    bicubic = torch.clamp(bicubic, 0, 1)

    # Nearest upsample LR for visualization
    lr_vis = F.interpolate(lr, scale_factor=2, mode='nearest')

    # Stack horizontally: LR | Bicubic | SR | HR
    grid = torch.cat([lr_vis, bicubic, sr, hr], dim=3)  # Concat along width

    # Convert to PIL for annotation
    grid_np = (grid * 255).clamp(0, 255).byte()
    # Stack samples vertically
    rows = []
    for i in range(num_samples):
        row_tensor = grid_np[i]  # (C, H, W)
        row_np = row_tensor.permute(1, 2, 0).numpy()  # (H, W, C)
        rows.append(row_np)
    
    # Combine all rows vertically
    combined = np.vstack(rows)
    img = Image.fromarray(combined, mode='RGB')
    
    # Draw labels
    draw = ImageDraw.Draw(img)
    
    # Try to load a font, fall back to default if unavailable
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 20)
    except (IOError, OSError):
        try:
            font = ImageFont.truetype("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", 20)
        except (IOError, OSError):
            font = ImageFont.load_default()
    
    # Image width for each panel
    panel_width = hr.shape[3]
    row_height = hr.shape[2]
    labels = ["LR (Input)", "Bicubic", "Super-Res", "HR (Target)"]
    
    # Draw labels on each row
    for row_idx in range(num_samples):
        y_offset = row_idx * row_height
        for col_idx, label in enumerate(labels):
            x_pos = col_idx * panel_width + 10
            y_pos = y_offset + 10
            # Draw text with outline for visibility
            outline_color = (0, 0, 0)
            text_color = (255, 255, 255)
            for dx, dy in [(-1, -1), (-1, 1), (1, -1), (1, 1), (-2, 0), (2, 0), (0, -2), (0, 2)]:
                draw.text((x_pos + dx, y_pos + dy), label, font=font, fill=outline_color)
            draw.text((x_pos, y_pos), label, font=font, fill=text_color)

    # Save
    save_path = os.path.join(out_dir, f'sample_step_{step:06d}.png')
    img.save(save_path)
    print(f"Saved sample to {save_path}")


def save_sample_grid_unsupervised(
    x: torch.Tensor,
    sr: torch.Tensor,
    step: int,
    out_dir: str,
    num_samples: int = 4,
) -> None:
    """Save a grid showing Input, Bicubic 4x, and SR output with labels (no target)."""
    from PIL import ImageDraw, ImageFont

    x = x[:num_samples].cpu()
    sr = sr[:num_samples].cpu()
    num_samples = x.shape[0]  # Actual batch size after slicing

    # Bicubic upsample input 4x for comparison
    bicubic = F.interpolate(x, scale_factor=4, mode='bicubic', align_corners=False)
    bicubic = torch.clamp(bicubic, 0, 1)

    # Nearest upsample input for visualization (to match SR size)
    x_vis = F.interpolate(x, scale_factor=4, mode='nearest')

    # Stack horizontally: Input | Bicubic | SR
    grid = torch.cat([x_vis, bicubic, sr], dim=3)  # Concat along width

    # Convert to PIL for annotation
    grid_np = (grid * 255).clamp(0, 255).byte()
    # Stack samples vertically
    rows = []
    for i in range(num_samples):
        row_tensor = grid_np[i]  # (C, H, W)
        row_np = row_tensor.permute(1, 2, 0).numpy()  # (H, W, C)
        rows.append(row_np)
    
    # Combine all rows vertically
    combined = np.vstack(rows)
    img = Image.fromarray(combined, mode='RGB')
    
    # Draw labels
    draw = ImageDraw.Draw(img)
    
    # Try to load a font, fall back to default if unavailable
    try:
        font = ImageFont.truetype("/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf", 20)
    except (IOError, OSError):
        try:
            font = ImageFont.truetype("/usr/share/fonts/TTF/DejaVuSans-Bold.ttf", 20)
        except (IOError, OSError):
            font = ImageFont.load_default()
    
    # Image width for each panel (512 pixels each)
    panel_width = sr.shape[3]
    row_height = sr.shape[2]
    labels = ["Input (nearest)", "Bicubic 4x", "Super-Res 4x"]
    
    # Draw labels on each row
    for row_idx in range(num_samples):
        y_offset = row_idx * row_height
        for col_idx, label in enumerate(labels):
            x_pos = col_idx * panel_width + 10
            y_pos = y_offset + 10
            # Draw text with outline for visibility
            outline_color = (0, 0, 0)
            text_color = (255, 255, 255)
            for dx, dy in [(-1, -1), (-1, 1), (1, -1), (1, 1), (-2, 0), (2, 0), (0, -2), (0, 2)]:
                draw.text((x_pos + dx, y_pos + dy), label, font=font, fill=outline_color)
            draw.text((x_pos, y_pos), label, font=font, fill=text_color)

    # Save
    save_path = os.path.join(out_dir, f'sample_step_{step:06d}.png')
    img.save(save_path)
    print(f"Saved sample to {save_path}")


# =============================================================================
# Inference Sharpening Utilities
# =============================================================================

def unsharp_mask(
    img: torch.Tensor, 
    kernel_size: int = 5, 
    sigma: float = 1.0, 
    amount: float = 1.0,
    threshold: float = 0.0,
) -> torch.Tensor:
    """
    Apply unsharp masking for inference-time sharpening.
    
    Args:
        img: Input tensor (B, C, H, W) in [0, 1]
        kernel_size: Gaussian blur kernel size
        sigma: Gaussian blur sigma
        amount: Sharpening strength (1.0 = normal, >1.0 = stronger)
        threshold: Minimum brightness change to apply sharpening
        
    Returns:
        Sharpened image tensor
    """
    # Create Gaussian kernel
    x = torch.arange(kernel_size, device=img.device, dtype=img.dtype) - (kernel_size - 1) / 2
    kernel_1d = torch.exp(-x ** 2 / (2 * sigma ** 2))
    kernel_1d = kernel_1d / kernel_1d.sum()
    kernel_2d = kernel_1d.unsqueeze(0) * kernel_1d.unsqueeze(1)
    kernel_2d = kernel_2d.expand(img.shape[1], 1, kernel_size, kernel_size)
    
    # Blur image
    padding = kernel_size // 2
    blurred = F.conv2d(img, kernel_2d, padding=padding, groups=img.shape[1])
    
    # Unsharp mask: original + amount * (original - blurred)
    mask = img - blurred
    
    if threshold > 0:
        # Only apply to regions with sufficient contrast
        mask = torch.where(torch.abs(mask) > threshold, mask, torch.zeros_like(mask))
    
    sharpened = img + amount * mask
    return torch.clamp(sharpened, 0, 1)


def self_ensemble(
    generator: nn.Module,
    lr: torch.Tensor,
    device: torch.device,
) -> torch.Tensor:
    """
    Self-ensemble (test-time augmentation) for improved quality.
    
    Generates 8 predictions with rotations/flips and averages them.
    
    Args:
        generator: Super-resolution model
        lr: Low-resolution input (B, C, H, W)
        device: torch device
        
    Returns:
        Averaged super-resolved output
    """
    generator.eval()
    outputs = []
    
    with torch.no_grad():
        for flip_h in [False, True]:
            for flip_v in [False, True]:
                for rot in [0, 1]:  # 0° and 90°
                    # Apply transforms
                    x = lr
                    if flip_h:
                        x = torch.flip(x, dims=[3])
                    if flip_v:
                        x = torch.flip(x, dims=[2])
                    if rot:
                        x = torch.rot90(x, k=1, dims=[2, 3])
                    
                    # Generate SR
                    sr = generator(x.to(device))
                    
                    # Reverse transforms
                    if rot:
                        sr = torch.rot90(sr, k=-1, dims=[2, 3])
                    if flip_v:
                        sr = torch.flip(sr, dims=[2])
                    if flip_h:
                        sr = torch.flip(sr, dims=[3])
                    
                    outputs.append(sr.cpu())
    
    # Average all predictions
    return torch.stack(outputs, dim=0).mean(dim=0)


def enhance_inference(
    generator: nn.Module,
    lr: torch.Tensor,
    device: torch.device,
    use_ensemble: bool = True,
    sharpen_amount: float = 0.3,
) -> torch.Tensor:
    """
    Enhanced inference with optional self-ensemble and sharpening.
    
    Args:
        generator: Super-resolution model
        lr: Low-resolution input
        device: torch device
        use_ensemble: Whether to use self-ensemble (8x slower but better)
        sharpen_amount: Amount of unsharp mask sharpening (0 = none)
        
    Returns:
        Enhanced super-resolved output
    """
    if use_ensemble:
        sr = self_ensemble(generator, lr, device)
    else:
        generator.eval()
        with torch.no_grad():
            sr = generator(lr.to(device)).cpu()
    
    if sharpen_amount > 0:
        sr = unsharp_mask(sr, amount=sharpen_amount)
    
    return sr


# =============================================================================
# Training
# =============================================================================

def train(args: argparse.Namespace) -> None:
    """Main training function."""
    # Set random seeds
    random.seed(42)
    np.random.seed(42)
    torch.manual_seed(42)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(42)

    # Device setup
    device = torch.device(args.device if torch.cuda.is_available() else 'cpu')
    print(f"Using device: {device}")

    if torch.cuda.is_available():
        print(f"GPU: {torch.cuda.get_device_name(0)}")
        print(f"Total VRAM: {torch.cuda.get_device_properties(0).total_memory / 1e9:.2f} GB")
        print(f"Currently allocated: {torch.cuda.memory_allocated() / 1e9:.2f} GB")

    # Create output directory
    os.makedirs(args.out_dir, exist_ok=True)

    # Load slide metadata
    print(f"Loading slide metadata from {args.csv}")
    slides = load_csv(args.csv)
    print(f"Loaded {len(slides)} slides")

    if len(slides) == 0:
        raise ValueError("No valid slides found in CSV (need at least one 128x128 tile)")

    # Create dataset based on data source
    if args.data_root is not None:
        # Use OpenSlide with local TIF files
        print(f"Creating dataset with OpenSlide from: {args.data_root}")
        if not OPENSLIDE_AVAILABLE:
            raise ImportError(
                "openslide-python is required for --data-root. "
                "Install with: pip install openslide-python"
            )
        dataset = OpenSlideTileDataset(
            slides=slides,
            data_root=args.data_root,
            train_level=args.train_level,
            tiles_per_slide=max(1, args.num_steps // len(slides)),
            color_jitter=args.color_jitter,
            color_jitter_strength=args.color_jitter_strength,
        )
    else:
        # Use gRPC
        # Test gRPC connection before starting training
        if not check_grpc_health(args.grpc_address):
            print("WARNING: gRPC health check failed. Continuing anyway...")
            print("If you continue to see connection errors, check:")
            print("  1. Is the storage server running?")
            print("  2. Is kubectl port-forward active?")
            print(f"  3. Is {args.grpc_address} the correct address?")

        # Create dataset and dataloader
        print(f"Creating dataset with gRPC address: {args.grpc_address}")
        dataset = GrpcTileDataset(
            slides=slides,
            grpc_address=args.grpc_address,
            train_level=args.train_level,
            tiles_per_slide=max(1, args.num_steps // len(slides)),
            color_jitter=args.color_jitter,
            color_jitter_strength=args.color_jitter_strength,
        )

    dataloader = DataLoader(
        dataset,
        batch_size=args.batch_size,
        shuffle=True,
        num_workers=args.num_workers,
        pin_memory=True,
        drop_last=True,
    )

    # TensorBoard writer with unique run directory
    from datetime import datetime
    if args.run_name:
        run_name = args.run_name
    else:
        run_name = datetime.now().strftime("%Y%m%d_%H%M%S")
    run_log_dir = os.path.join(args.log_dir, run_name)
    os.makedirs(run_log_dir, exist_ok=True)
    writer = SummaryWriter(log_dir=run_log_dir)
    print(f"TensorBoard logs: {run_log_dir}")

    # Create models
    print("Initializing models...")
    generator = Generator(
        base_channels=args.g_channels,
        num_residual_blocks=args.g_blocks,
        use_rrdb=args.use_rrdb,
        growth_channels=args.growth_channels,
    ).to(device)

    discriminator = Discriminator(
        base_channels=args.d_channels,
    ).to(device)

    # Count parameters
    g_params = sum(p.numel() for p in generator.parameters())
    d_params = sum(p.numel() for p in discriminator.parameters())
    print(f"Generator parameters: {g_params:,}")
    print(f"Discriminator parameters: {d_params:,}")

    # Create loss functions
    perceptual_loss_fn = None
    if VGG_AVAILABLE and args.lambda_perceptual > 0:
        perceptual_loss_fn = VGGPerceptualLoss().to(device).eval()
        print("Initialized VGG perceptual loss")
    elif args.lambda_perceptual > 0:
        print("WARNING: VGG not available, perceptual loss disabled")
        args.lambda_perceptual = 0
    
    edge_loss_fn = EdgeLoss().to(device)
    freq_loss_fn = FrequencyLoss(focus_high_freq=True, high_freq_weight=2.0).to(device)
    laplacian_loss_fn = LaplacianLoss().to(device)
    print("Initialized edge, frequency, and Laplacian sharpness losses")

    # Optimizers
    optimizer_g = torch.optim.AdamW(
        generator.parameters(),
        lr=args.lr,
        betas=(0.9, 0.999),
        weight_decay=1e-4,
    )
    optimizer_d = torch.optim.AdamW(
        discriminator.parameters(),
        lr=args.lr,
        betas=(0.9, 0.999),
        weight_decay=1e-4,
    )

    # Mixed precision scaler
    scaler = GradScaler()

    # Training state
    global_step = 0
    data_iter = iter(dataloader)

    # Loss weights
    lambda_pixel = args.lambda_pixel
    lambda_perceptual = args.lambda_perceptual
    lambda_edge = args.lambda_edge
    lambda_freq = args.lambda_freq
    lambda_adv = args.lambda_adv

    print(f"\nStarting training for {args.num_steps} steps")
    print(f"Pretrain steps (no adversarial): {args.pretrain_steps}")
    print(f"Loss weights: cycle={lambda_pixel}, perceptual={lambda_perceptual}, "
          f"edge={lambda_edge}, freq={lambda_freq}, adv={lambda_adv}")
    print("-" * 60)

    # Fixed batch for sampling
    sample_batch = None

    while global_step < args.num_steps:
        # Get next batch (returns full-res tiles only)
        try:
            x = next(data_iter)
        except StopIteration:
            data_iter = iter(dataloader)
            x = next(data_iter)

        x = x.to(device, non_blocking=True)

        # Save first batch for sampling
        if sample_batch is None:
            sample_batch = x.clone()

        # Pretraining phase (no adversarial loss)
        if global_step < args.pretrain_steps:
            generator.train()

            optimizer_g.zero_grad()

            with autocast():
                # Forward pass: input (128x128) -> SR (512x512)
                sr = generator(x)
                
                # Downsample SR back to original resolution for cycle consistency
                sr_down = F.interpolate(sr, scale_factor=0.25, mode='bicubic', align_corners=False)
                sr_down = torch.clamp(sr_down, 0, 1)

                # Cycle consistency loss: downsampled SR should match original input
                loss_cycle = F.l1_loss(sr_down, x)
                
                # Perceptual loss on downsampled SR vs input (if available)
                loss_perceptual = torch.tensor(0.0, device=device)
                if perceptual_loss_fn is not None:
                    loss_perceptual = perceptual_loss_fn(sr_down, x)
                
                # Edge and frequency losses on downsampled SR vs input
                loss_edge = edge_loss_fn(sr_down, x)
                loss_freq = freq_loss_fn(sr_down, x)

                loss_g = (
                    lambda_pixel * loss_cycle +
                    lambda_perceptual * loss_perceptual +
                    lambda_edge * loss_edge +
                    lambda_freq * loss_freq
                )

            # Backward pass
            scaler.scale(loss_g).backward()
            scaler.step(optimizer_g)
            scaler.update()

            # Logging
            if global_step % args.log_interval == 0:
                print(
                    f"[Pretrain] Step {global_step}/{args.num_steps} | "
                    f"L_cycle: {loss_cycle.item():.4f} | "
                    f"L_perc: {loss_perceptual.item():.4f} | "
                    f"L_edge: {loss_edge.item():.4f} | "
                    f"L_freq: {loss_freq.item():.4f}"
                )
                writer.add_scalar('Loss/cycle', loss_cycle.item(), global_step)
                writer.add_scalar('Loss/perceptual', loss_perceptual.item(), global_step)
                writer.add_scalar('Loss/edge', loss_edge.item(), global_step)
                writer.add_scalar('Loss/frequency', loss_freq.item(), global_step)
                writer.add_scalar('Loss/generator_total', loss_g.item(), global_step)

        # Adversarial phase
        else:
            generator.train()
            discriminator.train()

            # -------------------------
            # Update Discriminator
            # -------------------------
            optimizer_d.zero_grad()

            with autocast():
                # Generate SR (detached for D update)
                with torch.no_grad():
                    sr = generator(x)
                    # Downsample SR to input resolution for discrimination
                    sr_down = F.interpolate(sr, scale_factor=0.25, mode='bicubic', align_corners=False)
                    sr_down = torch.clamp(sr_down, 0, 1)

                # Real = original input tiles at full resolution
                # Fake = downsampled SR (should look like real tiles)
                d_real = discriminator(F.interpolate(x, scale_factor=4, mode='bicubic', align_corners=False))
                d_fake = discriminator(sr.detach())

                loss_d = discriminator_loss(d_real, d_fake)

            scaler.scale(loss_d).backward()
            scaler.step(optimizer_d)

            # -------------------------
            # Update Generator
            # -------------------------
            optimizer_g.zero_grad()

            with autocast():
                # Forward pass: input (128x128) -> SR (512x512)
                sr = generator(x)
                
                # Downsample SR back to original resolution
                sr_down = F.interpolate(sr, scale_factor=0.25, mode='bicubic', align_corners=False)
                sr_down = torch.clamp(sr_down, 0, 1)

                # Discriminator on generated SR
                d_fake = discriminator(sr)

                # Cycle consistency loss
                loss_cycle = F.l1_loss(sr_down, x)
                
                # Perceptual loss on downsampled SR vs input
                loss_perceptual = torch.tensor(0.0, device=device)
                if perceptual_loss_fn is not None:
                    loss_perceptual = perceptual_loss_fn(sr_down, x)
                
                # Edge and frequency losses
                loss_edge = edge_loss_fn(sr_down, x)
                loss_freq = freq_loss_fn(sr_down, x)
                loss_adv_g = generator_adversarial_loss(d_fake)

                loss_g = (
                    lambda_pixel * loss_cycle +
                    lambda_perceptual * loss_perceptual +
                    lambda_edge * loss_edge +
                    lambda_freq * loss_freq +
                    lambda_adv * loss_adv_g
                )

            scaler.scale(loss_g).backward()
            scaler.step(optimizer_g)
            scaler.update()

            # Logging
            if global_step % args.log_interval == 0:
                print(
                    f"Step {global_step}/{args.num_steps} | "
                    f"L_cycle: {loss_cycle.item():.4f} | "
                    f"L_perc: {loss_perceptual.item():.4f} | "
                    f"L_edge: {loss_edge.item():.4f} | "
                    f"L_adv: {loss_adv_g.item():.4f} | "
                    f"L_D: {loss_d.item():.4f}"
                )
                writer.add_scalar('Loss/cycle', loss_cycle.item(), global_step)
                writer.add_scalar('Loss/perceptual', loss_perceptual.item(), global_step)
                writer.add_scalar('Loss/edge', loss_edge.item(), global_step)
                writer.add_scalar('Loss/frequency', loss_freq.item(), global_step)
                writer.add_scalar('Loss/adversarial', loss_adv_g.item(), global_step)
                writer.add_scalar('Loss/generator_total', loss_g.item(), global_step)
                writer.add_scalar('Loss/discriminator', loss_d.item(), global_step)

        # VRAM logging
        if global_step % 1000 == 0:
            log_vram_usage(args.max_vram_gb)

        # Log images to TensorBoard
        if global_step > 0 and global_step % args.log_images_interval == 0:
            generator.eval()
            with torch.no_grad():
                sr_sample = generator(sample_batch)
                # Input (upsampled for comparison with SR)
                x_up = F.interpolate(sample_batch, scale_factor=4, mode='bicubic', align_corners=False)
                x_up = torch.clamp(x_up, 0, 1)
                # Downsampled SR (should match input) - resize for comparison grid
                sr_down = F.interpolate(sr_sample, scale_factor=0.25, mode='bicubic', align_corners=False)
                sr_down = torch.clamp(sr_down, 0, 1)
                sr_down_up = F.interpolate(sr_down, scale_factor=4, mode='bicubic', align_corners=False)
                # Create comparison grids (at SR resolution)
                input_grid = make_grid(x_up[:4].cpu(), nrow=2, normalize=False)
                output_grid = make_grid(sr_sample[:4].cpu(), nrow=2, normalize=False)
                cycle_grid = make_grid(sr_down_up[:4].cpu(), nrow=2, normalize=False)
                writer.add_image('Images/input_bicubic_4x', input_grid, global_step)
                writer.add_image('Images/output_sr', output_grid, global_step)
                writer.add_image('Images/cycle_reconstructed', cycle_grid, global_step)
            generator.train()

        # Save samples
        if global_step > 0 and global_step % args.sample_interval == 0:
            generator.eval()
            with torch.no_grad():
                sr_sample = generator(sample_batch)
            save_sample_grid_unsupervised(
                sample_batch, sr_sample, 
                global_step, args.out_dir
            )
            generator.train()

        # Save checkpoint
        if global_step > 0 and global_step % args.save_interval == 0:
            checkpoint_path = os.path.join(
                args.out_dir,
                f'checkpoint_step{global_step:06d}.pt'
            )
            torch.save({
                'step': global_step,
                'generator_state_dict': generator.state_dict(),
                'discriminator_state_dict': discriminator.state_dict(),
                'optimizer_g_state_dict': optimizer_g.state_dict(),
                'optimizer_d_state_dict': optimizer_d.state_dict(),
                'scaler_state_dict': scaler.state_dict(),
            }, checkpoint_path)
            print(f"Saved checkpoint to {checkpoint_path}")

        global_step += 1

    # Final checkpoint
    final_path = os.path.join(args.out_dir, 'checkpoint_final.pt')
    torch.save({
        'step': global_step,
        'generator_state_dict': generator.state_dict(),
        'discriminator_state_dict': discriminator.state_dict(),
        'optimizer_g_state_dict': optimizer_g.state_dict(),
        'optimizer_d_state_dict': optimizer_d.state_dict(),
        'scaler_state_dict': scaler.state_dict(),
    }, final_path)
    print(f"Training complete! Final checkpoint saved to {final_path}")

    # Close TensorBoard writer
    writer.close()


# =============================================================================
# Main
# =============================================================================

def main():
    parser = argparse.ArgumentParser(
        description='Train pathology super-resolution model',
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )

    # Required arguments
    parser.add_argument(
        '--csv',
        type=str,
        default='slides.csv',
        help='Path to CSV file with columns: slide_id,width_px,height_px',
    )

    # Data source settings (mutually exclusive: --data-root or --grpc-address)
    parser.add_argument(
        '--data-root',
        type=str,
        default=None,
        help='Path to directory containing TIF slide files (uses OpenSlide). '
             'Files should be named <slide_id>.tif. If set, gRPC is not used.',
    )
    parser.add_argument(
        '--grpc-address',
        type=str,
        default='localhost:8080',
        help='Address of the StorageApi gRPC server (ignored if --data-root is set)',
    )
    parser.add_argument(
        '--train-level',
        type=int,
        default=0,
        help='Mip level to request from StorageApi (0 = highest magnification)',
    )

    # Output settings
    parser.add_argument(
        '--out-dir',
        type=str,
        default='./checkpoints',
        help='Directory to save checkpoints and sample images',
    )

    # Training hyperparameters
    parser.add_argument(
        '--batch-size',
        type=int,
        default=8,
        help='Training batch size (reduce if OOM)',
    )
    parser.add_argument(
        '--num-steps',
        type=int,
        default=200000,
        help='Total number of training steps',
    )
    parser.add_argument(
        '--lr',
        type=float,
        default=1e-4,
        help='Learning rate for Adam optimizer',
    )
    parser.add_argument(
        '--pretrain-steps',
        type=int,
        default=500,
        help='Steps to train generator without adversarial loss',
    )

    # Loss weights
    parser.add_argument(
        '--lambda-pixel',
        type=float,
        default=1.0,
        help='Weight for pixel-wise L1 loss against HR ground truth',
    )
    parser.add_argument(
        '--lambda-perceptual',
        type=float,
        default=0.1,
        help='Weight for VGG perceptual loss (texture preservation)',
    )
    parser.add_argument(
        '--lambda-edge',
        type=float,
        default=0.1,
        help='Weight for edge/gradient sharpness loss',
    )
    parser.add_argument(
        '--lambda-freq',
        type=float,
        default=0.05,
        help='Weight for FFT frequency loss (high-freq detail)',
    )
    parser.add_argument(
        '--lambda-adv',
        type=float,
        default=0.01,
        help='Weight for adversarial loss',
    )

    # Model architecture
    parser.add_argument(
        '--g-channels',
        type=int,
        default=128,
        help='Base channels for generator (reduce for lower VRAM)',
    )
    parser.add_argument(
        '--g-blocks',
        type=int,
        default=20,
        help='Number of RRDB/residual blocks in generator (reduce for lower VRAM)',
    )
    parser.add_argument(
        '--use-rrdb',
        action='store_true',
        default=True,
        help='Use RRDB blocks (ESRGAN-style) for better quality',
    )
    parser.add_argument(
        '--no-rrdb',
        dest='use_rrdb',
        action='store_false',
        help='Use standard residual blocks instead of RRDB',
    )
    parser.add_argument(
        '--growth-channels',
        type=int,
        default=32,
        help='Growth channels for RRDB dense blocks (reduce for lower VRAM)',
    )
    parser.add_argument(
        '--d-channels',
        type=int,
        default=64,
        help='Base channels for discriminator (reduce for lower VRAM)',
    )

    # Data augmentation
    parser.add_argument(
        '--color-jitter',
        action='store_true',
        help='Enable light color jitter augmentation',
    )
    parser.add_argument(
        '--color-jitter-strength',
        type=float,
        default=0.05,
        help='Strength of color jitter augmentation',
    )

    # System settings
    parser.add_argument(
        '--device',
        type=str,
        default='cuda',
        help='Device to use for training (cuda or cpu)',
    )
    parser.add_argument(
        '--num-workers',
        type=int,
        default=6,
        help='Number of DataLoader workers',
    )
    parser.add_argument(
        '--max-vram-gb',
        type=float,
        default=27.5,
        help='Soft VRAM budget in GB (will log warnings if exceeded)',
    )

    # Checkpointing and sampling
    parser.add_argument(
        '--sample-interval',
        type=int,
        default=1000,
        help='Steps between saving sample SR images',
    )
    parser.add_argument(
        '--save-interval',
        type=int,
        default=5000,
        help='Steps between saving model checkpoints',
    )

    # TensorBoard logging
    parser.add_argument(
        '--log-dir',
        type=str,
        default='./runs',
        help='Base directory for TensorBoard logs (each run gets a timestamped subdirectory)',
    )
    parser.add_argument(
        '--run-name',
        type=str,
        default=None,
        help='Custom name for this run (used as subdirectory name). If not specified, uses timestamp.',
    )
    parser.add_argument(
        '--log-interval',
        type=int,
        default=100,
        help='Steps between logging metrics to TensorBoard',
    )
    parser.add_argument(
        '--log-images-interval',
        type=int,
        default=1000,
        help='Steps between logging image pairs to TensorBoard',
    )

    args = parser.parse_args()
    train(args)


if __name__ == '__main__':
    main()
