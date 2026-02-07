#!/usr/bin/env python3
"""
Production-Quality Variational Autoencoder (VAE) in PyTorch

A robust, well-structured convolutional VAE implementation with:
  - Spatial latent representation (not flattened)
  - Numerically stable reparameterization
  - KL annealing for training stability
  - Mixed precision training support
  - Clean separation between model, loss, and training loop

Default: 3x512x512 input images, configurable channels and resolution.
"""

import argparse
import math
import os
import random
import warnings
from dataclasses import dataclass, field
from typing import Any, Callable, Dict, List, Optional, Tuple, Union

import numpy as np
import torch
import torch.nn as nn
import torch.nn.functional as F
from torch.utils.data import DataLoader, Dataset

try:
    from torch.amp import autocast, GradScaler  # new style
    AMP_AVAILABLE = True
except ImportError:
    AMP_AVAILABLE = False
    GradScaler = None

try:
    from torch.utils.tensorboard import SummaryWriter
    TENSORBOARD_AVAILABLE = True
except ImportError:
    TENSORBOARD_AVAILABLE = False
    SummaryWriter = None

try:
    from torchvision.utils import make_grid
    from torchvision import transforms
    TORCHVISION_AVAILABLE = True
except ImportError:
    TORCHVISION_AVAILABLE = False
    transforms = None

try:
    import openslide
    OPENSLIDE_AVAILABLE = True
except ImportError:
    OPENSLIDE_AVAILABLE = False
    openslide = None

try:
    from PIL import Image
    PIL_AVAILABLE = True
except ImportError:
    PIL_AVAILABLE = False
    Image = None

import glob


# =============================================================================
# Configuration
# =============================================================================

@dataclass
class VAEConfig:
    """Configuration dataclass for VAE model and training."""

    # Image dimensions
    img_channels: int = 3
    img_size: int = 256

    # Model architecture (adjusted for anti-collapse: stronger latent, weaker decoder)
    base_channels: int = 32
    channel_multipliers: Tuple[int, ...] = (1, 2, 4, 4)
    latent_channels: int = 16
    downsample_factor: int = 16  # Total downsampling (256 / 16 = 16x16 latent)
    num_res_blocks_per_stage: int = 1
    use_attention_at: Tuple[int, ...] = ()  # Disabled to weaken decoder
    attention_num_heads: int = 4

    # Normalization and activation
    norm_type: str = "group"  # "group" or "layer"
    norm_num_groups: int = 8  # Must divide base_channels
    activation: str = "silu"  # "silu", "leaky_relu", "relu"

    # Training parameters
    beta: float = 1.0  # Maximum KL weight (β-VAE)
    kl_warmup_steps: int = 2000  # Steps per cycle for CyclicKLScheduler
    recon_loss_type: str = "l1"  # "l1" or "l2"
    learning_rate: float = 1e-4
    weight_decay: float = 0.01
    betas: Tuple[float, float] = (0.9, 0.999)

    # Stability
    max_grad_norm: Optional[float] = 1.0  # Gradient clipping, None to disable
    use_amp: bool = True  # Mixed precision training

    # Latent dimensions (computed)
    @property
    def latent_size(self) -> int:
        """Spatial size of the latent representation."""
        return self.img_size // self.downsample_factor

    def validate(self) -> None:
        """Validate configuration consistency."""
        # Check that downsample_factor matches channel_multipliers
        expected_downsample = 2 ** len(self.channel_multipliers)
        if self.downsample_factor != expected_downsample:
            warnings.warn(
                f"downsample_factor ({self.downsample_factor}) should typically equal "
                f"2^len(channel_multipliers) = {expected_downsample}"
            )

        # Check image size is divisible by downsample_factor
        if self.img_size % self.downsample_factor != 0:
            raise ValueError(
                f"img_size ({self.img_size}) must be divisible by "
                f"downsample_factor ({self.downsample_factor})"
            )


# =============================================================================
# Building Blocks
# =============================================================================

def get_activation(name: str) -> nn.Module:
    """Get activation function by name."""
    if name == "silu":
        return nn.SiLU()
    elif name == "leaky_relu":
        return nn.LeakyReLU(0.2)
    elif name == "relu":
        return nn.ReLU()
    else:
        raise ValueError(f"Unknown activation: {name}")


def get_norm_layer(
    num_channels: int,
    norm_type: str = "group",
    num_groups: int = 32,
) -> nn.Module:
    """Get normalization layer."""
    if norm_type == "group":
        # Ensure num_groups divides num_channels
        num_groups = min(num_groups, num_channels)
        while num_channels % num_groups != 0:
            num_groups -= 1
        return nn.GroupNorm(num_groups=num_groups, num_channels=num_channels, eps=1e-6)
    elif norm_type == "layer":
        return nn.GroupNorm(num_groups=1, num_channels=num_channels, eps=1e-6)
    elif norm_type == "batch":
        return nn.BatchNorm2d(num_channels)
    else:
        raise ValueError(f"Unknown norm_type: {norm_type}")


class ResidualBlock(nn.Module):
    """
    Residual block with GroupNorm and configurable activation.

    Args:
        in_channels: Number of input channels
        out_channels: Number of output channels
        norm_type: Type of normalization ("group", "layer")
        norm_num_groups: Number of groups for GroupNorm
        activation: Activation function name
        dropout: Dropout probability (0 to disable)
    """

    def __init__(
        self,
        in_channels: int,
        out_channels: int,
        norm_type: str = "group",
        norm_num_groups: int = 32,
        activation: str = "silu",
        dropout: float = 0.0,
    ):
        super().__init__()

        self.in_channels = in_channels
        self.out_channels = out_channels

        # First norm + conv
        self.norm1 = get_norm_layer(in_channels, norm_type, norm_num_groups)
        self.act1 = get_activation(activation)
        self.conv1 = nn.Conv2d(in_channels, out_channels,
                               kernel_size=3, padding=1)

        # Second norm + conv
        self.norm2 = get_norm_layer(out_channels, norm_type, norm_num_groups)
        self.act2 = get_activation(activation)
        self.dropout = nn.Dropout(dropout) if dropout > 0 else nn.Identity()
        self.conv2 = nn.Conv2d(out_channels, out_channels,
                               kernel_size=3, padding=1)

        # Skip connection (1x1 conv if channels differ)
        if in_channels != out_channels:
            self.skip = nn.Conv2d(in_channels, out_channels, kernel_size=1)
        else:
            self.skip = nn.Identity()

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        h = x
        h = self.norm1(h)
        h = self.act1(h)
        h = self.conv1(h)

        h = self.norm2(h)
        h = self.act2(h)
        h = self.dropout(h)
        h = self.conv2(h)

        return h + self.skip(x)


class SelfAttention2d(nn.Module):
    """
    Self-attention layer for 2D feature maps.

    Applies multi-head self-attention across spatial dimensions.
    Designed for use at low spatial resolutions (e.g., 64x64) to manage
    computational cost.

    Args:
        channels: Number of input/output channels
        num_heads: Number of attention heads
        norm_type: Type of normalization
        norm_num_groups: Number of groups for GroupNorm
    """

    def __init__(
        self,
        channels: int,
        num_heads: int = 4,
        norm_type: str = "group",
        norm_num_groups: int = 32,
    ):
        super().__init__()

        self.channels = channels
        self.num_heads = num_heads
        self.head_dim = channels // num_heads

        assert channels % num_heads == 0, \
            f"channels ({channels}) must be divisible by num_heads ({num_heads})"

        self.norm = get_norm_layer(channels, norm_type, norm_num_groups)

        # QKV projection
        self.qkv = nn.Conv2d(channels, channels * 3, kernel_size=1)

        # Output projection
        self.proj = nn.Conv2d(channels, channels, kernel_size=1)

        self.scale = self.head_dim ** -0.5

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        B, C, H, W = x.shape

        # Normalize input
        h = self.norm(x)

        # Compute Q, K, V
        qkv = self.qkv(h)  # (B, 3*C, H, W)
        qkv = qkv.reshape(B, 3, self.num_heads, self.head_dim, H * W)
        qkv = qkv.permute(1, 0, 2, 4, 3)  # (3, B, num_heads, H*W, head_dim)
        q, k, v = qkv[0], qkv[1], qkv[2]  # Each: (B, num_heads, H*W, head_dim)

        # Attention scores
        attn = torch.matmul(q, k.transpose(-2, -1)) * \
            self.scale  # (B, num_heads, H*W, H*W)
        attn = F.softmax(attn, dim=-1)

        # Apply attention to values
        out = torch.matmul(attn, v)  # (B, num_heads, H*W, head_dim)
        out = out.permute(0, 1, 3, 2).reshape(B, C, H, W)

        # Output projection + residual
        out = self.proj(out)
        return x + out


class Downsample(nn.Module):
    """2x spatial downsampling with strided convolution."""

    def __init__(self, channels: int):
        super().__init__()
        self.conv = nn.Conv2d(channels, channels,
                              kernel_size=3, stride=2, padding=1)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        return self.conv(x)


class Upsample(nn.Module):
    """2x spatial upsampling with nearest neighbor + convolution."""

    def __init__(self, channels: int):
        super().__init__()
        self.conv = nn.Conv2d(channels, channels, kernel_size=3, padding=1)

    def forward(self, x: torch.Tensor) -> torch.Tensor:
        x = F.interpolate(x, scale_factor=2.0, mode="nearest")
        return self.conv(x)


# =============================================================================
# Encoder
# =============================================================================

class Encoder(nn.Module):
    """
    VAE Encoder: Maps input images to latent distribution parameters (mu, logvar).

    Architecture:
        - Initial conv to base_channels
        - Multiple stages, each with:
            - res_blocks: ResidualBlock layers
            - Optional self-attention (at specified resolutions)
            - Downsample (except last stage)
        - Final conv to output mu and logvar

    Args:
        in_channels: Number of input image channels
        base_channels: Base channel count
        channel_multipliers: Channel multipliers for each stage
        latent_channels: Number of latent channels
        num_res_blocks: Number of residual blocks per stage
        use_attention_at: Spatial sizes where self-attention is applied
        attention_num_heads: Number of attention heads
        norm_type: Normalization type
        norm_num_groups: Number of groups for GroupNorm
        activation: Activation function name
    """

    def __init__(
        self,
        in_channels: int = 3,
        base_channels: int = 64,
        channel_multipliers: Tuple[int, ...] = (1, 2, 4, 8),
        latent_channels: int = 4,
        num_res_blocks: int = 2,
        use_attention_at: Tuple[int, ...] = (64,),
        attention_num_heads: int = 4,
        norm_type: str = "group",
        norm_num_groups: int = 32,
        activation: str = "silu",
    ):
        super().__init__()

        self.in_channels = in_channels
        self.latent_channels = latent_channels
        self.use_attention_at = set(use_attention_at)

        # Initial convolution
        self.conv_in = nn.Conv2d(
            in_channels, base_channels, kernel_size=3, padding=1)

        # Downsampling stages
        self.stages = nn.ModuleList()

        in_ch = base_channels
        current_resolution = None  # Will be set during forward based on input

        for i, mult in enumerate(channel_multipliers):
            out_ch = base_channels * mult
            stage = nn.ModuleDict()

            # Residual blocks
            blocks = nn.ModuleList()
            for j in range(num_res_blocks):
                blocks.append(ResidualBlock(
                    in_channels=in_ch if j == 0 else out_ch,
                    out_channels=out_ch,
                    norm_type=norm_type,
                    norm_num_groups=norm_num_groups,
                    activation=activation,
                ))
            stage["blocks"] = blocks

            # Attention (added dynamically based on resolution)
            # Placeholder, actual attention created if needed
            stage["attn"] = None

            # Downsample (except for last stage)
            if i < len(channel_multipliers) - 1:
                stage["downsample"] = Downsample(out_ch)
            else:
                stage["downsample"] = None

            self.stages.append(stage)
            in_ch = out_ch

        # Store channel info for attention creation
        self._channel_multipliers = channel_multipliers
        self._base_channels = base_channels
        self._attention_num_heads = attention_num_heads
        self._norm_type = norm_type
        self._norm_num_groups = norm_num_groups

        # Final normalization and output conv
        final_ch = base_channels * channel_multipliers[-1]
        self.norm_out = get_norm_layer(final_ch, norm_type, norm_num_groups)
        self.act_out = get_activation(activation)

        # Output: mu and logvar with same spatial dimensions
        # We output 2 * latent_channels and split later
        self.conv_out = nn.Conv2d(
            final_ch, 2 * latent_channels, kernel_size=3, padding=1)

        # Create attention modules where needed
        self._create_attention_modules()

    def _create_attention_modules(self) -> None:
        """Create attention modules for specified resolutions."""
        # This is called after __init__ to create attention modules
        # We track which stages need attention based on resolution
        for i, (stage, mult) in enumerate(zip(self.stages, self._channel_multipliers)):
            ch = self._base_channels * mult
            # Attention module (will be checked during forward)
            stage["attn"] = SelfAttention2d(
                channels=ch,
                num_heads=min(self._attention_num_heads, ch // 8),
                norm_type=self._norm_type,
                norm_num_groups=self._norm_num_groups,
            )

    def forward(self, x: torch.Tensor) -> Tuple[torch.Tensor, torch.Tensor]:
        """
        Encode input to latent distribution parameters.

        Args:
            x: Input tensor of shape (B, C, H, W)

        Returns:
            mu: Mean of latent distribution, shape (B, latent_channels, H', W')
            logvar: Log variance, shape (B, latent_channels, H', W')
        """
        # Initial conv
        h = self.conv_in(x)

        # Track current resolution for attention decisions
        current_res = x.shape[-1]

        # Process through stages
        for i, stage in enumerate(self.stages):
            # Residual blocks
            for block in stage["blocks"]:
                h = block(h)

            # Attention (if at specified resolution)
            if current_res in self.use_attention_at and stage["attn"] is not None:
                h = stage["attn"](h)

            # Downsample
            if stage["downsample"] is not None:
                h = stage["downsample"](h)
                current_res = current_res // 2

        # Final processing
        h = self.norm_out(h)
        h = self.act_out(h)
        h = self.conv_out(h)

        # Split into mu and logvar
        mu, logvar = h.chunk(2, dim=1)

        return mu, logvar


# =============================================================================
# Decoder
# =============================================================================

class Decoder(nn.Module):
    """
    VAE Decoder: Maps latent samples to reconstructed images.

    Architecture mirrors the encoder in reverse:
        - Initial conv from latent_channels
        - Multiple stages, each with:
            - res_blocks: ResidualBlock layers
            - Optional self-attention (at specified resolutions)
            - Upsample (except first stage)
        - Final conv to output image

    Args:
        out_channels: Number of output image channels
        base_channels: Base channel count
        channel_multipliers: Channel multipliers for each stage (reversed internally)
        latent_channels: Number of latent channels
        num_res_blocks: Number of residual blocks per stage
        use_attention_at: Spatial sizes where self-attention is applied
        attention_num_heads: Number of attention heads
        norm_type: Normalization type
        norm_num_groups: Number of groups for GroupNorm
        activation: Activation function name
    """

    def __init__(
        self,
        out_channels: int = 3,
        base_channels: int = 64,
        channel_multipliers: Tuple[int, ...] = (1, 2, 4, 8),
        latent_channels: int = 4,
        num_res_blocks: int = 2,
        use_attention_at: Tuple[int, ...] = (64,),
        attention_num_heads: int = 4,
        norm_type: str = "group",
        norm_num_groups: int = 32,
        activation: str = "silu",
    ):
        super().__init__()

        self.out_channels = out_channels
        self.latent_channels = latent_channels
        self.use_attention_at = set(use_attention_at)

        # Reverse channel multipliers for decoder
        channel_multipliers_rev = tuple(reversed(channel_multipliers))

        # Initial conv from latent
        first_ch = base_channels * channel_multipliers_rev[0]
        self.conv_in = nn.Conv2d(
            latent_channels, first_ch, kernel_size=3, padding=1)

        # Store for later
        self._channel_multipliers_rev = channel_multipliers_rev
        self._base_channels = base_channels
        self._attention_num_heads = attention_num_heads
        self._norm_type = norm_type
        self._norm_num_groups = norm_num_groups

        # Upsampling stages
        self.stages = nn.ModuleList()

        in_ch = first_ch

        for i, mult in enumerate(channel_multipliers_rev):
            out_ch = base_channels * mult
            stage = nn.ModuleDict()

            # Residual blocks
            blocks = nn.ModuleList()
            for j in range(num_res_blocks):
                blocks.append(ResidualBlock(
                    in_channels=in_ch if j == 0 else out_ch,
                    out_channels=out_ch,
                    norm_type=norm_type,
                    norm_num_groups=norm_num_groups,
                    activation=activation,
                ))
            stage["blocks"] = blocks

            # Attention placeholder
            stage["attn"] = None

            # Upsample (except for last stage)
            if i < len(channel_multipliers_rev) - 1:
                next_mult = channel_multipliers_rev[i + 1]
                next_ch = base_channels * next_mult
                stage["upsample"] = nn.Sequential(
                    Upsample(out_ch),
                    nn.Conv2d(out_ch, next_ch, kernel_size=3, padding=1),
                )
                in_ch = next_ch
            else:
                stage["upsample"] = None
                in_ch = out_ch

            self.stages.append(stage)

        # Final normalization and output conv
        final_ch = base_channels * channel_multipliers_rev[-1]
        self.norm_out = get_norm_layer(final_ch, norm_type, norm_num_groups)
        self.act_out = get_activation(activation)
        self.conv_out = nn.Conv2d(
            final_ch, out_channels, kernel_size=3, padding=1)

        # Create attention modules
        self._create_attention_modules()

    def _create_attention_modules(self) -> None:
        """Create attention modules for specified resolutions."""
        for i, (stage, mult) in enumerate(zip(self.stages, self._channel_multipliers_rev)):
            ch = self._base_channels * mult
            stage["attn"] = SelfAttention2d(
                channels=ch,
                num_heads=min(self._attention_num_heads, ch // 8),
                norm_type=self._norm_type,
                norm_num_groups=self._norm_num_groups,
            )

    def forward(self, z: torch.Tensor, target_size: Optional[int] = None) -> torch.Tensor:
        """
        Decode latent sample to image.

        Args:
            z: Latent tensor of shape (B, latent_channels, H', W')
            target_size: Optional target output size (for validation)

        Returns:
            x_recon: Reconstructed image, shape (B, out_channels, H, W)
        """
        # Initial conv
        h = self.conv_in(z)

        # Track current resolution
        current_res = z.shape[-1]

        # Process through stages
        for i, stage in enumerate(self.stages):
            # Residual blocks
            for block in stage["blocks"]:
                h = block(h)

            # Attention (if at specified resolution)
            if current_res in self.use_attention_at and stage["attn"] is not None:
                h = stage["attn"](h)

            # Upsample
            if stage["upsample"] is not None:
                h = stage["upsample"](h)
                current_res = current_res * 2

        # Final processing
        h = self.norm_out(h)
        h = self.act_out(h)
        x_recon = self.conv_out(h)

        # Tanh to ensure output is in [-1, 1] range for images
        x_recon = torch.tanh(x_recon)

        return x_recon


# =============================================================================
# VAE Model
# =============================================================================

class VAE(nn.Module):
    """
    Variational Autoencoder with spatial latent representation.

    Combines encoder and decoder, implements reparameterization trick,
    and computes VAE loss (reconstruction + KL divergence).

    Args:
        config: VAEConfig instance with all hyperparameters

    Alternatively, individual parameters can be passed:
        in_channels, img_size, base_channels, channel_multipliers,
        latent_channels, num_res_blocks, use_attention_at, etc.
    """

    def __init__(
        self,
        config: Optional[VAEConfig] = None,
        # Individual params (used if config is None)
        in_channels: int = 3,
        img_size: int = 512,
        base_channels: int = 64,
        channel_multipliers: Tuple[int, ...] = (1, 2, 4, 8),
        latent_channels: int = 4,
        num_res_blocks: int = 2,
        use_attention_at: Tuple[int, ...] = (64,),
        attention_num_heads: int = 4,
        norm_type: str = "group",
        norm_num_groups: int = 32,
        activation: str = "silu",
        recon_loss_type: str = "l1",
    ):
        super().__init__()

        # Use config if provided, otherwise use individual params
        if config is not None:
            in_channels = config.img_channels
            img_size = config.img_size
            base_channels = config.base_channels
            channel_multipliers = config.channel_multipliers
            latent_channels = config.latent_channels
            num_res_blocks = config.num_res_blocks_per_stage
            use_attention_at = config.use_attention_at
            attention_num_heads = config.attention_num_heads
            norm_type = config.norm_type
            norm_num_groups = config.norm_num_groups
            activation = config.activation
            recon_loss_type = config.recon_loss_type

        self.in_channels = in_channels
        self.img_size = img_size
        self.latent_channels = latent_channels
        self.recon_loss_type = recon_loss_type

        # Build encoder
        self.encoder = Encoder(
            in_channels=in_channels,
            base_channels=base_channels,
            channel_multipliers=channel_multipliers,
            latent_channels=latent_channels,
            num_res_blocks=num_res_blocks,
            use_attention_at=use_attention_at,
            attention_num_heads=attention_num_heads,
            norm_type=norm_type,
            norm_num_groups=norm_num_groups,
            activation=activation,
        )

        # Build decoder
        self.decoder = Decoder(
            out_channels=in_channels,
            base_channels=base_channels,
            channel_multipliers=channel_multipliers,
            latent_channels=latent_channels,
            num_res_blocks=num_res_blocks,
            use_attention_at=use_attention_at,
            attention_num_heads=attention_num_heads,
            norm_type=norm_type,
            norm_num_groups=norm_num_groups,
            activation=activation,
        )

    def encode(self, x: torch.Tensor) -> Tuple[torch.Tensor, torch.Tensor]:
        """
        Encode input to latent distribution parameters.

        Args:
            x: Input tensor of shape (B, C, H, W)

        Returns:
            mu: Mean of latent distribution
            logvar: Log variance of latent distribution
        """
        return self.encoder(x)

    def decode(self, z: torch.Tensor) -> torch.Tensor:
        """
        Decode latent sample to image.

        Args:
            z: Latent tensor of shape (B, latent_channels, H', W')

        Returns:
            x_recon: Reconstructed image
        """
        return self.decoder(z)

    def reparameterize(self, mu, logvar, training: bool = True):
        # Much tighter clamp
        logvar = torch.clamp(logvar, min=-10.0, max=5.0)  # std in ~[0.0067, ~4.5]
        mu = torch.clamp(mu, min=-10.0, max=10.0)

        if training:
            std = torch.exp(0.5 * logvar)
            eps = torch.randn_like(std)
            return mu + eps * std
        else:
            return mu


    def forward(
        self,
        x: torch.Tensor,
        kl_weight: float = 1.0,
        return_latent: bool = False,
    ) -> Dict[str, torch.Tensor]:
        """
        Forward pass: encode, reparameterize, decode, compute losses.

        Args:
            x: Input tensor of shape (B, C, H, W), expected in [-1, 1] or [0, 1]
            kl_weight: Weight for KL loss term (for β-VAE / KL annealing)
            return_latent: If True, include latent tensors in output

        Returns:
            Dict containing:
                - x_recon: Reconstructed image
                - loss: Total loss (recon + kl_weight * kl)
                - recon_loss: Reconstruction loss
                - kl_loss: KL divergence (unweighted)
                - mu: Latent mean (if return_latent)
                - logvar: Latent log variance (if return_latent)
                - z: Sampled latent (if return_latent)
        """
        # Clamp input to valid range to prevent NaN propagation
        x = torch.clamp(x, min=-1.0, max=1.0)
        
        # Encode
        mu, logvar = self.encode(x)

        # Reparameterize
        z = self.reparameterize(mu, logvar, training=self.training)
        
        # Clamp z to prevent extreme values
        z = torch.clamp(z, min=-10.0, max=10.0)

        # Decode
        x_recon = self.decode(z)

        # Clamp reconstruction to valid range to prevent NaN propagation
        x_recon = torch.clamp(x_recon, min=-1.0, max=1.0)

        # Compute losses
        recon_loss = reconstruction_loss(
            x, x_recon, loss_type=self.recon_loss_type)
        kl_loss = kl_divergence(mu, logvar)

        # Clamp individual losses to prevent extreme values
        recon_loss = torch.clamp(recon_loss, min=0.0, max=100.0)
        kl_loss = torch.clamp(kl_loss, min=0.0, max=1000.0)

        # Total loss with KL weighting
        loss = recon_loss + kl_weight * kl_loss

        # Build output dict
        output = {
            "x_recon": x_recon,
            "loss": loss,
            "recon_loss": recon_loss,
            "kl_loss": kl_loss,
        }

        if return_latent:
            output["mu"] = mu
            output["logvar"] = logvar
            output["z"] = z

        return output

    def sample(
        self,
        num_samples: int,
        device: torch.device,
        latent_size: Optional[int] = None,
    ) -> torch.Tensor:
        """
        Sample new images from the prior.

        Args:
            num_samples: Number of samples to generate
            device: Device to generate on
            latent_size: Spatial size of latent (if None, computed from img_size)

        Returns:
            samples: Generated images
        """
        if latent_size is None:
            # Compute based on encoder downsampling
            latent_size = self.img_size // (2 ** len(self.encoder.stages))

        # Type narrowing for type checker
        lat_size: int = latent_size

        # Sample from prior (standard normal)
        z = torch.randn(
            num_samples,
            self.latent_channels,
            lat_size,
            lat_size,
            device=device,
        )

        # Decode
        with torch.no_grad():
            samples = self.decode(z)

        return samples

    def reconstruct(self, x: torch.Tensor, deterministic: bool = False) -> torch.Tensor:
        """
        Reconstruct input images.

        Args:
            x: Input images
            deterministic: If True, use mean instead of sampling

        Returns:
            x_recon: Reconstructed images
        """
        mu, logvar = self.encode(x)
        z = self.reparameterize(mu, logvar, training=not deterministic)
        return self.decode(z)


# =============================================================================
# Loss Functions
# =============================================================================

def kl_divergence(mu, logvar, free_nats: float = 0.5):
    # Match clamp here too
    logvar = torch.clamp(logvar, min=-10.0, max=5.0)
    mu = torch.clamp(mu, min=-10.0, max=10.0)

    kl = -0.5 * (1 + logvar - mu.pow(2) - logvar.exp())
    kl = kl.sum(dim=[1, 2, 3])
    kl = torch.clamp(kl, min=free_nats)
    kl = torch.nan_to_num(kl, nan=free_nats, posinf=1e3, neginf=free_nats)
    return kl.mean()


def reconstruction_loss(
    x: torch.Tensor,
    x_recon: torch.Tensor,
    loss_type: str = "l1",
) -> torch.Tensor:
    """
    Compute reconstruction loss in pixel space.

    Args:
        x: Original input, shape (B, C, H, W)
        x_recon: Reconstructed output, shape (B, C, H, W)
        loss_type: "l1" for L1 loss, "l2" for MSE

    Returns:
        Reconstruction loss (scalar)
    """
    if loss_type == "l1":
        return F.l1_loss(x_recon, x, reduction="mean")
    elif loss_type == "l2":
        return F.mse_loss(x_recon, x, reduction="mean")
    else:
        raise ValueError(f"Unknown loss_type: {loss_type}")


# =============================================================================
# KL Scheduler
# =============================================================================

class LinearKLScheduler:
    """
    Linear KL weight warmup scheduler.

    Increases KL weight from 0 to beta over warmup_steps.

    Args:
        beta: Maximum KL weight
        warmup_steps: Number of steps to reach beta
    """

    def __init__(self, beta: float, warmup_steps: int):
        self.beta = beta
        self.warmup_steps = warmup_steps

    def __call__(self, global_step: int) -> float:
        """Get current KL weight."""
        if self.warmup_steps <= 0:
            return self.beta

        progress = min(1.0, global_step / self.warmup_steps)
        return self.beta * progress

    def state_dict(self) -> Dict[str, Any]:
        return {"beta": self.beta, "warmup_steps": self.warmup_steps}

    def load_state_dict(self, state_dict: Dict[str, Any]) -> None:
        self.beta = state_dict["beta"]
        self.warmup_steps = state_dict["warmup_steps"]


class CyclicKLScheduler:
    """
    Cyclic KL annealing scheduler.

    Repeats the warmup cycle multiple times during training.
    This can help with posterior collapse.

    Args:
        beta: Maximum KL weight
        cycle_steps: Steps per cycle
        ratio: Fraction of cycle for warmup (rest is at beta)
    """

    def __init__(self, beta: float, cycle_steps: int, ratio: float = 0.5):
        self.beta = beta
        self.cycle_steps = cycle_steps
        self.ratio = ratio
        self.warmup_steps = int(cycle_steps * ratio)

    def __call__(self, global_step: int) -> float:
        """Get current KL weight."""
        step_in_cycle = global_step % self.cycle_steps

        if step_in_cycle < self.warmup_steps:
            return self.beta * step_in_cycle / self.warmup_steps
        else:
            return self.beta

    def state_dict(self) -> Dict[str, Any]:
        return {
            "beta": self.beta,
            "cycle_steps": self.cycle_steps,
            "ratio": self.ratio,
        }

    def load_state_dict(self, state_dict: Dict[str, Any]) -> None:
        self.beta = state_dict["beta"]
        self.cycle_steps = state_dict["cycle_steps"]
        self.ratio = state_dict["ratio"]
        self.warmup_steps = int(self.cycle_steps * self.ratio)


# =============================================================================
# Training Utilities
# =============================================================================

def check_for_nan(loss: torch.Tensor, name: str = "loss") -> bool:
    """
    Check for NaN or Inf in a tensor and print warning.

    Returns True if NaN/Inf detected.
    """
    if torch.isnan(loss).any() or torch.isinf(loss).any():
        print(f"WARNING: {name} contains NaN or Inf!")
        return True
    return False


def has_nonfinite_gradients(model: nn.Module) -> bool:
    """
    Return True if any parameter gradient has NaN or Inf.
    Also prints the first offending parameter name and stats.
    """
    for name, p in model.named_parameters():
        if p.grad is not None:
            if not torch.isfinite(p.grad).all():
                g = p.grad
                g_min = g.min().item()
                g_max = g.max().item()
                g_mean = g.mean().item()
                print(
                    f"Non-finite gradients detected in '{name}': "
                    f"min={g_min:.3e}, max={g_max:.3e}, mean={g_mean:.3e}"
                )
                return True
    return False



def train_epoch(
    model: VAE,
    dataloader: DataLoader,
    optimizer: torch.optim.Optimizer,
    device: torch.device,
    epoch: int,
    kl_scheduler: Union[LinearKLScheduler, CyclicKLScheduler],
    global_step: int,
    scaler: Optional[Any] = None,  # GradScaler
    max_grad_norm: Optional[float] = None,
    writer: Optional[Any] = None,  # SummaryWriter
    log_interval: int = 100,
    image_log_interval: int = 1000,
) -> Tuple[Dict[str, float], int]:
    """
    Train for one epoch.

    Args:
        model: VAE model
        dataloader: Training data loader
        optimizer: Optimizer
        device: Device to train on
        epoch: Current epoch number
        kl_scheduler: KL weight scheduler
        global_step: Current global step (updated in-place)
        scaler: GradScaler for mixed precision (None to disable)
        max_grad_norm: Max gradient norm for clipping (None to disable)
        writer: TensorBoard SummaryWriter (None to disable logging)
        log_interval: Steps between scalar logging
        image_log_interval: Steps between image logging

    Returns:
        Tuple of (metrics_dict, updated_global_step)
    """
    model.train()

    total_loss = 0.0
    total_recon_loss = 0.0
    total_kl_loss = 0.0
    num_batches = 0

    for batch_idx, batch in enumerate(dataloader):
        # Handle different batch formats
        if isinstance(batch, (list, tuple)):
            x = batch[0]
        else:
            x = batch

        x = x.to(device)

        # Get current KL weight
        kl_weight = kl_scheduler(global_step)

        # Forward pass with optional mixed precision
        optimizer.zero_grad()

        # Check input for NaN/Inf (can happen with bad tile data)
        if torch.isnan(x).any() or torch.isinf(x).any():
            print(f"Skipping batch {batch_idx} due to NaN/Inf in input data")
            continue

        if scaler is not None:
            # Forward pass with AMP
            with autocast(device_type="cuda", dtype=torch.float16):
                outputs = model(x, kl_weight=kl_weight, return_latent=True)
                loss = outputs["loss"]

            # If the loss itself is NaN, skip entirely without backward to avoid corrupting weights
            if check_for_nan(loss, "loss"):
                print(f"Skipping batch {batch_idx} due to NaN loss (forward)")
                optimizer.zero_grad(set_to_none=True)
                # Just reduce the scale without corrupting weights
                scaler.update(scaler.get_scale() * 0.5)
                continue

            scaler.scale(loss).backward()

            # Optional: gradient clipping
            if max_grad_norm is not None:
                # Unscareparameterizele once before clipping (official pattern)
                scaler.unscale_(optimizer)
                torch.nn.utils.clip_grad_norm_(model.parameters(), max_grad_norm)

            scaler.step(optimizer)  # internally checks for NaN/Inf grads and skips update if needed
            scaler.update()
        else:
            outputs = model(x, kl_weight=kl_weight, return_latent=True)
            loss = outputs["loss"]

            # Check for NaN
            if check_for_nan(loss, "loss"):
                print(f"Skipping batch {batch_idx} due to NaN loss")
                continue

            loss.backward()

            if has_nonfinite_gradients(model):
                print(
                    f"Skipping batch {batch_idx} due to non-finite gradients")
                optimizer.zero_grad(set_to_none=True)
                continue

            # Gradient clipping
            if max_grad_norm is not None:
                torch.nn.utils.clip_grad_norm_(
                    model.parameters(), max_grad_norm)

            optimizer.step()

        # Accumulate metrics
        total_loss += loss.item()
        total_recon_loss += outputs["recon_loss"].item()
        total_kl_loss += outputs["kl_loss"].item()
        num_batches += 1

        # Logging
        if writer is not None and global_step % log_interval == 0:
            writer.add_scalar("train/loss", loss.item(), global_step)
            writer.add_scalar("train/recon_loss",
                              outputs["recon_loss"].item(), global_step)
            writer.add_scalar(
                "train/kl_loss", outputs["kl_loss"].item(), global_step)
            writer.add_scalar("train/kl_weight", kl_weight, global_step)
            # Log mu and logvar histograms for diagnosing posterior collapse
            if "mu" in outputs and "logvar" in outputs:
                writer.add_histogram(
                    "train/mu", outputs["mu"].detach(), global_step)
                writer.add_histogram(
                    "train/logvar", outputs["logvar"].detach(), global_step)

        # Image logging
        if writer is not None and global_step % image_log_interval == 0:
            log_images(writer, x, outputs["x_recon"],
                       global_step, prefix="train")

        global_step += 1

    # Compute averages
    metrics = {
        "loss": total_loss / max(num_batches, 1),
        "recon_loss": total_recon_loss / max(num_batches, 1),
        "kl_loss": total_kl_loss / max(num_batches, 1),
    }

    return metrics, global_step


@torch.no_grad()
def evaluate(
    model: VAE,
    dataloader: DataLoader,
    device: torch.device,
    kl_weight: float = 1.0,
    max_batches: Optional[int] = None,
) -> Dict[str, float]:
    """
    Evaluate model on a dataset.

    Args:
        model: VAE model
        dataloader: Evaluation data loader
        device: Device to evaluate on
        kl_weight: KL weight to use
        max_batches: Maximum number of batches to evaluate (None for all)

    Returns:
        Dictionary of average metrics
    """
    model.eval()

    total_loss = 0.0
    total_recon_loss = 0.0
    total_kl_loss = 0.0
    num_batches = 0

    for batch_idx, batch in enumerate(dataloader):
        if max_batches is not None and batch_idx >= max_batches:
            break

        # Handle different batch formats
        if isinstance(batch, (list, tuple)):
            x = batch[0]
        else:
            x = batch

        x = x.to(device)

        outputs = model(x, kl_weight=kl_weight, return_latent=False)

        total_loss += outputs["loss"].item()
        total_recon_loss += outputs["recon_loss"].item()
        total_kl_loss += outputs["kl_loss"].item()
        num_batches += 1

    return {
        "loss": total_loss / max(num_batches, 1),
        "recon_loss": total_recon_loss / max(num_batches, 1),
        "kl_loss": total_kl_loss / max(num_batches, 1),
    }


def log_images(
    writer: Any,
    x: torch.Tensor,
    x_recon: torch.Tensor,
    step: int,
    prefix: str = "train",
    num_images: int = 4,
) -> None:
    """
    Log original and reconstructed images to TensorBoard.

    Args:
        writer: TensorBoard SummaryWriter
        x: Original images
        x_recon: Reconstructed images
        step: Global step
        prefix: Logging prefix
        num_images: Number of images to log
    """
    if not TORCHVISION_AVAILABLE:
        return

    # Take first few images
    x = x[:num_images].cpu()
    x_recon = x_recon[:num_images].cpu()

    # Convert from [-1, 1] to [0, 1] for visualization
    x = (x + 1.0) / 2.0
    x_recon = (x_recon + 1.0) / 2.0

    # Clamp to valid range
    x = torch.clamp(x, 0, 1)
    x_recon = torch.clamp(x_recon, 0, 1)

    # Create grids
    grid_orig = make_grid(x, nrow=num_images, normalize=False)
    grid_recon = make_grid(x_recon, nrow=num_images, normalize=False)

    # Combined grid
    combined = torch.cat([grid_orig, grid_recon], dim=1)

    writer.add_image(f"{prefix}/orig_vs_recon", combined, step)


# =============================================================================
# Utility Functions
# =============================================================================

def has_content(img: Image.Image, min_std: float = 5.0, min_mean: float = 10.0, max_mean: float = 245.0) -> bool:
    """
    Check if an image tile has meaningful content.

    TIF files are sparse, so many tiles may be completely black/white (empty).
    This function filters out empty tiles by checking pixel statistics.

    Args:
        img: PIL Image to check
        min_std: Minimum standard deviation threshold (indicates variation)
        min_mean: Minimum mean pixel value threshold (filters pure black)
        max_mean: Maximum mean pixel value threshold (filters pure white)

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
    # Check if image is not predominantly white (empty background)
    if arr.mean() > max_mean:
        return False
    return True


# =============================================================================
# OpenSlide Tile Dataset
# =============================================================================

class OpenSlideTileDataset(Dataset):
    """
    PyTorch Dataset that extracts random tiles from TIF files using OpenSlide.

    Scans a directory for .tif files and extracts random tiles.
    Images are returned as tensors in [0, 1] range.
    """

    def __init__(
        self,
        data_root: str,
        tile_size: int = 256,
        tiles_per_epoch: int = 10000,
        level: int = 0,
        color_jitter: bool = False,
        color_jitter_strength: float = 0.05,
    ):
        """
        Args:
            data_root: Directory containing .tif files
            tile_size: Size of tiles to extract (tile_size x tile_size)
            tiles_per_epoch: Number of tiles per epoch
            level: OpenSlide pyramid level to read from (0 = highest resolution)
            color_jitter: Apply random color jitter augmentation
            color_jitter_strength: Strength of color jitter
        """
        if not OPENSLIDE_AVAILABLE:
            raise ImportError(
                "openslide-python is required. Install with: pip install openslide-python"
            )

        self.data_root = data_root
        self.tile_size = tile_size
        self.tiles_per_epoch = tiles_per_epoch
        self.level = level

        # Find all .tif files in the directory
        self.tif_files = glob.glob(os.path.join(data_root, "*.tif"))
        self.tif_files += glob.glob(os.path.join(data_root, "*.TIF"))
        self.tif_files += glob.glob(os.path.join(data_root,
                                    "**", "*.tif"), recursive=True)
        self.tif_files += glob.glob(os.path.join(data_root,
                                    "**", "*.TIF"), recursive=True)
        # Remove duplicates
        self.tif_files = list(set(self.tif_files))

        if len(self.tif_files) == 0:
            raise ValueError(f"No .tif files found in {data_root}")

        print(f"Found {len(self.tif_files)} TIF files in {data_root}")

        # Cache for OpenSlide objects and dimensions (opened lazily)
        self._slide_cache: Dict[str, Any] = {}
        self._slide_dimensions: Dict[str, Tuple[int, int]] = {}
        self._invalid_slides: set = set()  # Track slides that failed to open

        # Transform to tensor
        self.to_tensor = transforms.ToTensor() if transforms else None

        # Color jitter augmentation
        if color_jitter and transforms:
            self.jitter = transforms.ColorJitter(
                brightness=color_jitter_strength,
                contrast=color_jitter_strength,
                saturation=color_jitter_strength,
                hue=color_jitter_strength * 0.5,
            )
        else:
            self.jitter = None

    def _get_slide_with_dims(self, tif_path: str) -> Optional[Tuple[Any, Tuple[int, int]]]:
        """Get or open an OpenSlide object and its dimensions. Returns None if invalid."""
        if tif_path in self._invalid_slides:
            return None

        if tif_path not in self._slide_cache:
            try:
                slide = openslide.OpenSlide(tif_path)
                # Get dimensions at the specified level
                level = min(self.level, slide.level_count - 1)
                dims = slide.level_dimensions[level]

                # Check if large enough for at least one tile
                if dims[0] < self.tile_size or dims[1] < self.tile_size:
                    slide.close()
                    self._invalid_slides.add(tif_path)
                    return None

                self._slide_cache[tif_path] = slide
                self._slide_dimensions[tif_path] = dims
            except Exception as e:
                print(f"Warning: Could not open {tif_path}: {e}")
                self._invalid_slides.add(tif_path)
                return None

        return self._slide_cache[tif_path], self._slide_dimensions[tif_path]

    def __len__(self) -> int:
        return self.tiles_per_epoch

    def _extract_random_tile(self, max_attempts: int = 50) -> Tuple[Optional[Image.Image], str]:
        """Extract a random tile from a random slide (lazy loading).

        Returns:
            Tuple of (image or None, debug_info string with last attempt details)
        """
        last_attempt_info = "no attempts made"
        empty_tile_count = 0
        open_error_count = 0

        for attempt in range(max_attempts):
            # Pick a random slide
            tif_path = random.choice(self.tif_files)

            # Get slide and dimensions (lazy loading)
            result = self._get_slide_with_dims(tif_path)
            if result is None:
                open_error_count += 1
                last_attempt_info = f"failed to open {os.path.basename(tif_path)}"
                continue

            slide, dims = result

            # Pick random coordinates
            max_x = dims[0] - self.tile_size
            max_y = dims[1] - self.tile_size

            if max_x <= 0 or max_y <= 0:
                last_attempt_info = f"{os.path.basename(tif_path)} too small ({dims[0]}x{dims[1]})"
                continue

            x = random.randint(0, max_x)
            y = random.randint(0, max_y)

            try:
                # Get downsample factor for level
                level = min(self.level, slide.level_count - 1)
                downsample = slide.level_downsamples[level]

                # Convert to level 0 coordinates
                level0_x = int(x * downsample)
                level0_y = int(y * downsample)

                # Read tile
                img = slide.read_region(
                    (level0_x, level0_y),
                    level,
                    (self.tile_size, self.tile_size)
                )

                # Convert RGBA to RGB with white background
                # OpenSlide returns RGBA where transparent areas (outside tissue) have alpha=0
                # Composite onto white background to avoid black regions
                if img.mode == 'RGBA':
                    background = Image.new('RGB', img.size, (255, 255, 255))
                    # Use alpha channel as mask
                    background.paste(img, mask=img.split()[3])
                    img = background
                else:
                    img = img.convert('RGB')

                # Hacky fix: replace near-black pixels with white
                # Some TIF backgrounds render as black (0,0,0) rather than transparent
                arr = np.array(img)
                near_black_mask = (arr[:, :, 0] < 4) & (
                    arr[:, :, 1] < 4) & (arr[:, :, 2] < 4)
                arr[near_black_mask] = [255, 255, 255]
                img = Image.fromarray(arr)

                # Check if tile has content (not empty/black/white background)
                if has_content(img):
                    return img, "success"
                else:
                    empty_tile_count += 1
                    last_attempt_info = f"{os.path.basename(tif_path)} at ({x},{y}) was empty (black/white/uniform)"

            except Exception as e:
                last_attempt_info = f"error reading {os.path.basename(tif_path)} at ({x},{y}): {e}"
                continue

        # Build detailed debug info
        debug_info = (
            f"Failed after {max_attempts} attempts. "
            f"Empty tiles: {empty_tile_count}, open errors: {open_error_count}. "
            f"Last: {last_attempt_info}"
        )
        return None, debug_info

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
        """Get a random tile. Always returns a valid tile."""
        # Keep trying until we get a valid tile
        total_attempts = 0
        max_total_attempts = 500  # Safety limit to avoid infinite loops

        while total_attempts < max_total_attempts:
            img, debug_info = self._extract_random_tile()
            total_attempts += 50  # Each call to _extract_random_tile does 50 attempts

            if img is not None:
                break

            # Log occasionally if we're having trouble
            if total_attempts % 100 == 0:
                print(
                    f"Warning: Struggled to find valid tile after {total_attempts} attempts. {debug_info}")

        if img is None:
            # This should basically never happen unless all slides are empty
            raise RuntimeError(
                f"Could not find a valid tile after {max_total_attempts} attempts. "
                f"Last error: {debug_info}. "
                f"Check that your TIF files contain non-empty regions."
            )

        # Apply augmentations
        img = self._apply_augmentations(img)

        # Convert to tensor [0, 1]
        if self.to_tensor is not None:
            tensor = self.to_tensor(img)
        else:
            # Manual conversion if torchvision not available
            arr = np.array(img, dtype=np.float32) / 255.0
            tensor = torch.from_numpy(arr).permute(2, 0, 1)

        # Map from [0, 1] to [-1, 1]
        tensor = tensor * 2.0 - 1.0

        return tensor

    def __del__(self):
        """Close all cached OpenSlide objects."""
        for slide in self._slide_cache.values():
            try:
                slide.close()
            except:
                pass


# =============================================================================
# Main Training Script
# =============================================================================

def parse_args() -> argparse.Namespace:
    """Parse command line arguments."""
    parser = argparse.ArgumentParser(description="Train VAE on image data")

    # Data
    parser.add_argument("--data-root", type=str, required=True,
                        help="Directory containing .tif files")
    parser.add_argument("--img-size", type=int, default=256,
                        help="Image/tile size (default: 256)")
    parser.add_argument("--img-channels", type=int, default=3,
                        help="Number of image channels")
    parser.add_argument("--batch-size", type=int, default=8,
                        help="Batch size")
    parser.add_argument("--num-workers", type=int, default=12,
                        help="Number of data loader workers")
    parser.add_argument("--tiles-per-epoch", type=int, default=10000,
                        help="Number of tiles per epoch")
    parser.add_argument("--level", type=int, default=0,
                        help="OpenSlide pyramid level (0=highest resolution)")

    # Model (~8M params with defaults: base=32, mults=1,2,4,8)
    parser.add_argument("--base-channels", type=int, default=32,
                        help="Base channel count (default: 32 for ~8M params)")
    parser.add_argument("--latent-channels", type=int, default=4,
                        help="Number of latent channels")
    parser.add_argument("--channel-multipliers", type=str, default="1,2,4,8",
                        help="Channel multipliers (comma-separated)")
    parser.add_argument("--num-res-blocks", type=int, default=2,
                        help="Residual blocks per stage")
    parser.add_argument("--use-attention-at", type=str, default="16",
                        help="Spatial sizes for attention (comma-separated)")

    # Training
    parser.add_argument("--epochs", type=int, default=100,
                        help="Number of training epochs")
    parser.add_argument("--lr", type=float, default=1e-4,
                        help="Learning rate")
    parser.add_argument("--weight-decay", type=float, default=0.01,
                        help="Weight decay")
    parser.add_argument("--beta", type=float, default=1.0,
                        help="Maximum KL weight (beta-VAE)")
    parser.add_argument("--kl-warmup-steps", type=int, default=5000,
                        help="Steps for KL warmup")
    parser.add_argument("--max-grad-norm", type=float, default=1.0,
                        help="Max gradient norm (0 to disable)")
    parser.add_argument("--recon-loss-type", type=str, default="l1",
                        choices=["l1", "l2"], help="Reconstruction loss type")

    # Mixed precision
    parser.add_argument("--use-amp", action="store_true", default=True,
                        help="Use mixed precision training")
    parser.add_argument("--no-amp", action="store_false", dest="use_amp",
                        help="Disable mixed precision training")

    # Logging
    parser.add_argument("--log-dir", type=str, default="runs_vae",
                        help="TensorBoard log directory")
    parser.add_argument("--checkpoint-dir", type=str, default="checkpoints_vae",
                        help="Checkpoint directory")
    parser.add_argument("--log-interval", type=int, default=100,
                        help="Steps between logging")
    parser.add_argument("--save-interval", type=int, default=5,
                        help="Epochs between checkpoints")

    # Device
    parser.add_argument("--device", type=str, default="cuda",
                        help="Device to use")

    # Reproducibility
    parser.add_argument("--seed", type=int, default=42,
                        help="Random seed")

    return parser.parse_args()


def main():
    """Main training function."""
    args = parse_args()

    # Set random seeds for reproducibility
    random.seed(args.seed)
    np.random.seed(args.seed)
    torch.manual_seed(args.seed)
    if torch.cuda.is_available():
        torch.cuda.manual_seed_all(args.seed)

    # Device setup
    device = torch.device(args.device if torch.cuda.is_available() else "cpu")
    print(f"Using device: {device}")

    if torch.cuda.is_available():
        print(f"GPU: {torch.cuda.get_device_name(0)}")
        print(
            f"Total VRAM: {torch.cuda.get_device_properties(0).total_memory / 1e9:.2f} GB")

    # Parse channel multipliers and attention resolutions
    channel_multipliers = tuple(int(x)
                                for x in args.channel_multipliers.split(","))
    use_attention_at = tuple(int(x) for x in args.use_attention_at.split(","))

    # Create config
    config = VAEConfig(
        img_channels=args.img_channels,
        img_size=args.img_size,
        base_channels=args.base_channels,
        channel_multipliers=channel_multipliers,
        latent_channels=args.latent_channels,
        num_res_blocks_per_stage=args.num_res_blocks,
        use_attention_at=use_attention_at,
        beta=args.beta,
        kl_warmup_steps=args.kl_warmup_steps,
        recon_loss_type=args.recon_loss_type,
        learning_rate=args.lr,
        weight_decay=args.weight_decay,
        max_grad_norm=args.max_grad_norm if args.max_grad_norm > 0 else None,
        use_amp=args.use_amp and AMP_AVAILABLE,
    )
    config.validate()

    print(f"\nVAE Configuration:")
    print(
        f"  Image size: {config.img_size}x{config.img_size}x{config.img_channels}")
    print(
        f"  Latent size: {config.latent_size}x{config.latent_size}x{config.latent_channels}")
    print(f"  Base channels: {config.base_channels}")
    print(f"  Channel multipliers: {config.channel_multipliers}")
    print(f"  Attention at: {config.use_attention_at}")
    print(f"  Beta (max KL weight): {config.beta}")
    print(f"  KL warmup steps: {config.kl_warmup_steps}")
    print(f"  Recon loss type: {config.recon_loss_type}")
    print(f"  Mixed precision: {config.use_amp}")
    print()

    # Create model
    model = VAE(config=config).to(device)

    # Count parameters
    num_params = sum(p.numel() for p in model.parameters())
    num_trainable = sum(p.numel()
                        for p in model.parameters() if p.requires_grad)
    print(f"Model parameters: {num_params:,} ({num_trainable:,} trainable)")

    # Create optimizer
    optimizer = torch.optim.AdamW(
        model.parameters(),
        lr=config.learning_rate,
        betas=config.betas,
        weight_decay=config.weight_decay,
    )

    # Create KL scheduler (cyclic annealing to prevent posterior collapse)
    kl_scheduler = CyclicKLScheduler(
        beta=config.beta,
        cycle_steps=config.kl_warmup_steps,
        ratio=0.5,  # First half of each cycle ramps KL from 0 -> beta
    )

    # Create grad scaler for mixed precision
    scaler = None
    if config.use_amp and AMP_AVAILABLE:
        scaler = GradScaler()
        print("Using mixed precision training (AMP)")

    # Create OpenSlide tile dataset
    print(f"\nCreating dataset from TIF files in: {args.data_root}")
    train_dataset = OpenSlideTileDataset(
        data_root=args.data_root,
        tile_size=config.img_size,
        tiles_per_epoch=args.tiles_per_epoch,
        level=args.level,
        color_jitter=True,
        color_jitter_strength=0.05,
    )

    # Use same dataset for validation (but fewer tiles)
    val_dataset = OpenSlideTileDataset(
        data_root=args.data_root,
        tile_size=config.img_size,
        tiles_per_epoch=args.tiles_per_epoch // 10,  # 10% for validation
        level=args.level,
        color_jitter=False,  # No augmentation for validation
    )

    train_loader = DataLoader(
        train_dataset,
        batch_size=args.batch_size,
        shuffle=True,
        num_workers=args.num_workers,
        pin_memory=True,
        drop_last=True,
    )

    val_loader = DataLoader(
        val_dataset,
        batch_size=args.batch_size,
        shuffle=False,
        num_workers=args.num_workers,
        pin_memory=True,
    )

    # Create directories
    os.makedirs(args.log_dir, exist_ok=True)
    os.makedirs(args.checkpoint_dir, exist_ok=True)

    # TensorBoard writer
    writer = None
    if TENSORBOARD_AVAILABLE:
        from datetime import datetime
        run_name = datetime.now().strftime("%Y%m%d_%H%M%S")
        log_path = os.path.join(args.log_dir, run_name)
        writer = SummaryWriter(log_dir=log_path)
        print(f"TensorBoard logs: {log_path}")

    # Training loop
    global_step = 0
    best_val_loss = float("inf")

    print(f"\nStarting training for {args.epochs} epochs...")
    print(f"Steps per epoch: {len(train_loader)}")
    print()

    for epoch in range(args.epochs):
        # Train
        train_metrics, global_step = train_epoch(
            model=model,
            dataloader=train_loader,
            optimizer=optimizer,
            device=device,
            epoch=epoch,
            kl_scheduler=kl_scheduler,
            global_step=global_step,
            scaler=scaler,
            max_grad_norm=config.max_grad_norm,
            writer=writer,
            log_interval=args.log_interval,
        )

        # Get current KL weight for reporting
        current_kl_weight = kl_scheduler(global_step)

        # Evaluate
        val_metrics = evaluate(
            model=model,
            dataloader=val_loader,
            device=device,
            kl_weight=current_kl_weight,
        )

        # Log epoch metrics
        if writer is not None:
            writer.add_scalar("epoch/train_loss", train_metrics["loss"], epoch)
            writer.add_scalar("epoch/train_recon_loss",
                              train_metrics["recon_loss"], epoch)
            writer.add_scalar("epoch/train_kl_loss",
                              train_metrics["kl_loss"], epoch)
            writer.add_scalar("epoch/val_loss", val_metrics["loss"], epoch)
            writer.add_scalar("epoch/val_recon_loss",
                              val_metrics["recon_loss"], epoch)
            writer.add_scalar("epoch/val_kl_loss",
                              val_metrics["kl_loss"], epoch)

        # Print progress
        print(
            f"Epoch {epoch + 1}/{args.epochs} | "
            f"Train Loss: {train_metrics['loss']:.4f} "
            f"(recon: {train_metrics['recon_loss']:.4f}, kl: {train_metrics['kl_loss']:.4f}) | "
            f"Val Loss: {val_metrics['loss']:.4f} | "
            f"KL weight: {current_kl_weight:.4f}"
        )

        # Save checkpoint
        if (epoch + 1) % args.save_interval == 0 or val_metrics["loss"] < best_val_loss:
            checkpoint = {
                "epoch": epoch,
                "global_step": global_step,
                "model_state_dict": model.state_dict(),
                "optimizer_state_dict": optimizer.state_dict(),
                "kl_scheduler_state_dict": kl_scheduler.state_dict(),
                "train_metrics": train_metrics,
                "val_metrics": val_metrics,
                "config": {
                    "img_channels": config.img_channels,
                    "img_size": config.img_size,
                    "base_channels": config.base_channels,
                    "channel_multipliers": config.channel_multipliers,
                    "latent_channels": config.latent_channels,
                    "beta": config.beta,
                },
            }

            if scaler is not None:
                checkpoint["scaler_state_dict"] = scaler.state_dict()

            # Save periodic checkpoint
            if (epoch + 1) % args.save_interval == 0:
                save_path = os.path.join(
                    args.checkpoint_dir, f"checkpoint_epoch_{epoch + 1}.pt")
                torch.save(checkpoint, save_path)
                print(f"Saved checkpoint: {save_path}")

            # Save best checkpoint
            if val_metrics["loss"] < best_val_loss:
                best_val_loss = val_metrics["loss"]
                save_path = os.path.join(
                    args.checkpoint_dir, "checkpoint_best.pt")
                torch.save(checkpoint, save_path)
                print(f"Saved best checkpoint: {save_path}")

    # Save final checkpoint
    checkpoint = {
        "epoch": args.epochs - 1,
        "global_step": global_step,
        "model_state_dict": model.state_dict(),
        "optimizer_state_dict": optimizer.state_dict(),
        "kl_scheduler_state_dict": kl_scheduler.state_dict(),
    }
    save_path = os.path.join(args.checkpoint_dir, "checkpoint_final.pt")
    torch.save(checkpoint, save_path)
    print(f"Saved final checkpoint: {save_path}")

    if writer is not None:
        writer.close()

    print("\nTraining complete!")


if __name__ == "__main__":
    main()
