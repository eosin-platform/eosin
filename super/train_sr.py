#!/usr/bin/env python3
"""
Pathology Super-Resolution Training Script

Trains a generator G that upsamples 40x histology tiles (512x512) to 2x resolution (1024x1024)
while preserving consistency: downsampling the SR image should reconstruct the original.
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

# Assume these are generated and importable
import storage_pb2
import storage_pb2_grpc


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
    max_tiles_x: int  # width_px // 512
    max_tiles_y: int  # height_px // 512


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
            max_tiles_x = width_px // 512
            max_tiles_y = height_px // 512
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
    """

    def __init__(
        self,
        slides: List[SlideMetadata],
        data_root: str,
        train_level: int = 0,
        target_tile_size: int = 512,
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
            # At the training level, each tile is 512x512 pixels
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

    def _apply_augmentations(self, img: Image.Image) -> torch.Tensor:
        """Apply augmentations and convert to tensor."""
        # Random horizontal flip
        if random.random() > 0.5:
            img = img.transpose(Image.FLIP_LEFT_RIGHT)

        # Random vertical flip
        if random.random() > 0.5:
            img = img.transpose(Image.FLIP_TOP_BOTTOM)

        # Random 90° rotations
        k = random.randint(0, 3)
        if k > 0:
            img = img.rotate(k * 90, expand=False)

        # Color jitter (optional)
        if self.jitter is not None:
            img = self.jitter(img)

        # Convert to tensor [0, 1]
        tensor = self.to_tensor(img)
        return tensor

    def __getitem__(self, idx: int) -> torch.Tensor:
        """Get a single tile sample."""
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

            # Check if tile is exactly 512x512
            if img.size != (self.target_tile_size, self.target_tile_size):
                continue

            # Apply augmentations and return
            tensor = self._apply_augmentations(img)
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
    """

    def __init__(
        self,
        slides: List[SlideMetadata],
        grpc_address: str,
        train_level: int = 0,
        target_tile_size: int = 512,
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

    def _apply_augmentations(self, img: Image.Image) -> torch.Tensor:
        """Apply augmentations and convert to tensor."""
        # Random horizontal flip
        if random.random() > 0.5:
            img = img.transpose(Image.FLIP_LEFT_RIGHT)

        # Random vertical flip
        if random.random() > 0.5:
            img = img.transpose(Image.FLIP_TOP_BOTTOM)

        # Random 90° rotations
        k = random.randint(0, 3)
        if k > 0:
            img = img.rotate(k * 90, expand=False)

        # Color jitter (optional)
        if self.jitter is not None:
            img = self.jitter(img)

        # Convert to tensor [0, 1]
        tensor = self.to_tensor(img)
        return tensor

    def __getitem__(self, idx: int) -> torch.Tensor:
        """Get a single tile sample."""
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

            # Check if tile is exactly 512x512
            if img.size != (self.target_tile_size, self.target_tile_size):
                continue

            # Apply augmentations and return
            tensor = self._apply_augmentations(img)
            return tensor

        # If we couldn't get a valid tile, try a different slide
        new_slide_idx = random.randint(0, len(self.slides) - 1)
        return self.__getitem__(new_slide_idx * self.tiles_per_slide)


# =============================================================================
# Model Components
# =============================================================================

class ResidualBlock(nn.Module):
    """Residual block with two convolutions."""

    def __init__(self, channels: int):
        super().__init__()
        self.conv1 = nn.Conv2d(channels, channels, 3, padding=1)
        self.bn1 = nn.BatchNorm2d(channels)
        self.conv2 = nn.Conv2d(channels, channels, 3, padding=1)
        self.bn2 = nn.BatchNorm2d(channels)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        residual = x
        out = F.relu(self.bn1(self.conv1(x)), inplace=True)
        out = self.bn2(self.conv2(out))
        return out + residual


class Generator(nn.Module):
    """
    Super-resolution generator.
    
    Takes a 512x512 input and produces a 1024x1024 output with hallucinated
    high-frequency details while preserving the original when downsampled.
    """

    def __init__(
        self,
        in_channels: int = 3,
        base_channels: int = 64,
        num_residual_blocks: int = 12,
        residual_scale: float = 0.5,
    ):
        super().__init__()
        self.residual_scale = residual_scale

        # Initial feature extraction
        self.conv_in = nn.Conv2d(in_channels, base_channels, 3, padding=1)

        # Residual blocks
        self.residual_blocks = nn.Sequential(
            *[ResidualBlock(base_channels) for _ in range(num_residual_blocks)]
        )

        # Post-residual conv
        self.conv_mid = nn.Conv2d(base_channels, base_channels, 3, padding=1)
        self.bn_mid = nn.BatchNorm2d(base_channels)

        # Upsampling via PixelShuffle (2x)
        self.upsample = nn.Sequential(
            nn.Conv2d(base_channels, base_channels * 4, 3, padding=1),
            nn.PixelShuffle(2),
            nn.ReLU(inplace=True),
        )

        # Final output conv for residual
        self.conv_out = nn.Sequential(
            nn.Conv2d(base_channels, base_channels, 3, padding=1),
            nn.ReLU(inplace=True),
            nn.Conv2d(base_channels, in_channels, 3, padding=1),
            nn.Tanh(),  # Output in [-1, 1], will be scaled
        )

    def forward(self, x: torch.Tensor) -> Tuple[torch.Tensor, torch.Tensor]:
        """
        Forward pass.
        
        Args:
            x: Input tensor of shape (B, 3, 512, 512) in [0, 1]
            
        Returns:
            y: Super-resolved output (B, 3, 1024, 1024)
            r: Residual component (B, 3, 1024, 1024)
        """
        # Bicubic upsample input to target resolution
        x_up = F.interpolate(x, scale_factor=2, mode='bicubic', align_corners=False)
        x_up = torch.clamp(x_up, 0, 1)

        # Extract features
        feat = F.relu(self.conv_in(x), inplace=True)

        # Residual learning
        res_feat = self.residual_blocks(feat)
        res_feat = self.bn_mid(self.conv_mid(res_feat))
        feat = feat + res_feat

        # Upsample features
        feat = self.upsample(feat)

        # Generate residual
        r = self.conv_out(feat)  # Output in [-1, 1]
        r = r * 0.5  # Scale to [-0.5, 0.5]

        # Final SR output: upsampled input + scaled residual
        y = x_up + self.residual_scale * r
        y = torch.clamp(y, 0, 1)

        return y, r


class Discriminator(nn.Module):
    """
    Patch-based discriminator for 1024x1024 images.
    
    Outputs a grid of real/fake scores.
    """

    def __init__(self, in_channels: int = 3, base_channels: int = 64):
        super().__init__()

        def conv_block(in_ch, out_ch, stride=2, bn=True):
            layers = [nn.Conv2d(in_ch, out_ch, 4, stride, 1, bias=not bn)]
            if bn:
                layers.append(nn.BatchNorm2d(out_ch))
            layers.append(nn.LeakyReLU(0.2, inplace=True))
            return nn.Sequential(*layers)

        self.model = nn.Sequential(
            # Input: (B, 3, 1024, 1024)
            conv_block(in_channels, base_channels, stride=2, bn=False),  # -> 512
            conv_block(base_channels, base_channels * 2, stride=2),       # -> 256
            conv_block(base_channels * 2, base_channels * 4, stride=2),   # -> 128
            conv_block(base_channels * 4, base_channels * 8, stride=2),   # -> 64
            conv_block(base_channels * 8, base_channels * 8, stride=2),   # -> 32
            conv_block(base_channels * 8, base_channels * 8, stride=1),   # -> 32
            nn.Conv2d(base_channels * 8, 1, 4, 1, 1),                     # -> 31x31 patch
        )

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        """
        Args:
            x: Input tensor of shape (B, 3, 1024, 1024)
            
        Returns:
            Patch-wise real/fake scores
        """
        return self.model(x)


# =============================================================================
# Loss Functions and Utilities
# =============================================================================

def downsample(y: torch.Tensor, scale_factor: float = 0.5) -> torch.Tensor:
    """Differentiable bicubic downsampling."""
    return F.interpolate(y, scale_factor=scale_factor, mode='bicubic', align_corners=False)


def upsample(x: torch.Tensor, scale_factor: float = 2.0) -> torch.Tensor:
    """Differentiable bicubic upsampling."""
    return F.interpolate(x, scale_factor=scale_factor, mode='bicubic', align_corners=False)


def reconstruction_loss(y: torch.Tensor, x: torch.Tensor) -> torch.Tensor:
    """L1 loss between downsampled SR and original."""
    y_down = downsample(y)
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
        raise ValueError("No valid slides found in CSV (need at least one 512x512 tile)")

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

    # TensorBoard writer
    os.makedirs(args.log_dir, exist_ok=True)
    writer = SummaryWriter(log_dir=args.log_dir)
    print(f"TensorBoard logs: {args.log_dir}")

    # Create models
    print("Initializing models...")
    generator = Generator(
        base_channels=args.g_channels,
        num_residual_blocks=args.g_blocks,
        residual_scale=args.residual_scale,
    ).to(device)

    discriminator = Discriminator(
        base_channels=args.d_channels,
    ).to(device)

    # Count parameters
    g_params = sum(p.numel() for p in generator.parameters())
    d_params = sum(p.numel() for p in discriminator.parameters())
    print(f"Generator parameters: {g_params:,}")
    print(f"Discriminator parameters: {d_params:,}")

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
    lambda_recon = args.lambda_recon
    lambda_adv = args.lambda_adv
    lambda_reg = args.lambda_reg

    print(f"\nStarting training for {args.num_steps} steps")
    print(f"Pretrain steps (no adversarial): {args.pretrain_steps}")
    print(f"Loss weights: recon={lambda_recon}, adv={lambda_adv}, reg={lambda_reg}")
    print("-" * 60)

    # Fixed batch for sampling
    sample_batch = None

    while global_step < args.num_steps:
        # Get next batch
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
                # Forward pass
                y, r = generator(x)

                # Losses
                loss_recon = reconstruction_loss(y, x)
                loss_reg = regularization_loss(r)
                loss_g = lambda_recon * loss_recon + lambda_reg * loss_reg

            # Backward pass
            scaler.scale(loss_g).backward()
            scaler.step(optimizer_g)
            scaler.update()

            # Logging
            if global_step % args.log_interval == 0:
                print(
                    f"[Pretrain] Step {global_step}/{args.num_steps} | "
                    f"L_recon: {loss_recon.item():.4f} | "
                    f"L_reg: {loss_reg.item():.4f}"
                )
                writer.add_scalar('Loss/reconstruction', loss_recon.item(), global_step)
                writer.add_scalar('Loss/regularization', loss_reg.item(), global_step)
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
                    y, _ = generator(x)

                # Real: bicubic-upsampled original
                x_up = upsample(x)
                x_up = torch.clamp(x_up, 0, 1)

                # Discriminator outputs
                d_real = discriminator(x_up)
                d_fake = discriminator(y.detach())

                loss_d = discriminator_loss(d_real, d_fake)

            scaler.scale(loss_d).backward()
            scaler.step(optimizer_d)

            # -------------------------
            # Update Generator
            # -------------------------
            optimizer_g.zero_grad()

            with autocast():
                # Forward pass
                y, r = generator(x)

                # Discriminator on fake
                d_fake = discriminator(y)

                # Losses
                loss_recon = reconstruction_loss(y, x)
                loss_reg = regularization_loss(r)
                loss_adv = generator_adversarial_loss(d_fake)

                loss_g = (
                    lambda_recon * loss_recon +
                    lambda_adv * loss_adv +
                    lambda_reg * loss_reg
                )

            scaler.scale(loss_g).backward()
            scaler.step(optimizer_g)
            scaler.update()

            # Logging
            if global_step % args.log_interval == 0:
                print(
                    f"Step {global_step}/{args.num_steps} | "
                    f"L_recon: {loss_recon.item():.4f} | "
                    f"L_adv: {loss_adv.item():.4f} | "
                    f"L_reg: {loss_reg.item():.4f} | "
                    f"L_D: {loss_d.item():.4f}"
                )
                writer.add_scalar('Loss/reconstruction', loss_recon.item(), global_step)
                writer.add_scalar('Loss/adversarial', loss_adv.item(), global_step)
                writer.add_scalar('Loss/regularization', loss_reg.item(), global_step)
                writer.add_scalar('Loss/generator_total', loss_g.item(), global_step)
                writer.add_scalar('Loss/discriminator', loss_d.item(), global_step)

        # VRAM logging
        if global_step % 1000 == 0:
            log_vram_usage(args.max_vram_gb)

        # Log images to TensorBoard
        if global_step > 0 and global_step % args.log_images_interval == 0:
            generator.eval()
            with torch.no_grad():
                y_sample, r_sample = generator(sample_batch)
                # Input images (upsampled for comparison)
                x_up = F.interpolate(sample_batch, scale_factor=2, mode='bicubic', align_corners=False)
                x_up = torch.clamp(x_up, 0, 1)
                # Create comparison grids
                input_grid = make_grid(x_up[:4].cpu(), nrow=2, normalize=False)
                output_grid = make_grid(y_sample[:4].cpu(), nrow=2, normalize=False)
                residual_grid = make_grid((r_sample[:4].cpu() + 0.5).clamp(0, 1), nrow=2, normalize=False)
                writer.add_image('Images/input_bicubic', input_grid, global_step)
                writer.add_image('Images/output_sr', output_grid, global_step)
                writer.add_image('Images/residual', residual_grid, global_step)
            generator.train()

        # Save samples
        if global_step > 0 and global_step % args.sample_interval == 0:
            generator.eval()
            with torch.no_grad():
                y_sample, _ = generator(sample_batch)
            save_sample_grid(sample_batch, y_sample, global_step, args.out_dir)
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
        default=4,
        help='Training batch size',
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
        default=2e-4,
        help='Learning rate for Adam optimizer',
    )
    parser.add_argument(
        '--pretrain-steps',
        type=int,
        default=5_000,
        help='Steps to train generator without adversarial loss',
    )

    # Loss weights
    parser.add_argument(
        '--lambda-recon',
        type=float,
        default=1.0,
        help='Weight for reconstruction loss',
    )
    parser.add_argument(
        '--lambda-adv',
        type=float,
        default=0.01,
        help='Weight for adversarial loss',
    )
    parser.add_argument(
        '--lambda-reg',
        type=float,
        default=0.001,
        help='Weight for residual regularization loss',
    )

    # Model architecture
    parser.add_argument(
        '--g-channels',
        type=int,
        default=64,
        help='Base channels for generator',
    )
    parser.add_argument(
        '--g-blocks',
        type=int,
        default=12,
        help='Number of residual blocks in generator',
    )
    parser.add_argument(
        '--d-channels',
        type=int,
        default=64,
        help='Base channels for discriminator',
    )
    parser.add_argument(
        '--residual-scale',
        type=float,
        default=0.5,
        help='Scale factor for residual addition (alpha)',
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
        default=4,
        help='Number of DataLoader workers',
    )
    parser.add_argument(
        '--max-vram-gb',
        type=float,
        default=25.0,
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
        help='Directory for TensorBoard logs',
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
