#!/usr/bin/env python3
"""
Hyperparameter Search for Pathology Super-Resolution

Uses Ray Tune to search over hyperparameters while keeping train_sr.py clean.
Wraps the train function with minimal modifications.

Usage:
    python hparam_search.py --csv slides.csv --grpc-address localhost:8080

Requirements:
    pip install "ray[tune]" optuna
"""

import argparse
import os
from argparse import Namespace
from typing import Any, Dict, Optional

import ray
from ray import tune
from ray.air import RunConfig, ScalingConfig
from ray.tune.schedulers import ASHAScheduler, PopulationBasedTraining
from ray.tune.search.optuna import OptunaSearch

# Import the train function from the main training script
from train_sr import train


def create_args_from_config(config: Dict[str, Any], base_args: Namespace) -> Namespace:
    """
    Create an args namespace by merging Ray Tune config with base args.
    
    Args:
        config: Ray Tune hyperparameter config
        base_args: Base arguments from command line
    
    Returns:
        Merged Namespace for training
    """
    # Start with base args
    args = Namespace(**vars(base_args))
    
    # Override with tuned hyperparameters
    for key, value in config.items():
        if hasattr(args, key.replace('-', '_')):
            setattr(args, key.replace('-', '_'), value)
        else:
            setattr(args, key, value)
    
    # Generate unique run name for this trial
    trial_id = tune.get_trial_id() if tune.is_session_started() else "local"
    args.run_name = f"hparam_{trial_id}"
    
    return args


def train_wrapper(config: Dict[str, Any], base_args: Namespace) -> None:
    """
    Wrapper function for Ray Tune that calls the original train function.
    
    Args:
        config: Hyperparameter configuration from Ray Tune
        base_args: Base arguments from command line
    """
    # Merge config with base args
    args = create_args_from_config(config, base_args)
    
    # Create callback that reports to Ray Tune
    def metric_callback(metrics: Dict[str, float], step: int) -> None:
        """Report metrics to Ray Tune."""
        # Report all metrics with step
        tune.report(
            step=step,
            loss_cycle=metrics.get('loss_cycle', 0.0),
            loss_perceptual=metrics.get('loss_perceptual', 0.0),
            loss_edge=metrics.get('loss_edge', 0.0),
            loss_frequency=metrics.get('loss_frequency', 0.0),
            loss_adversarial=metrics.get('loss_adversarial', 0.0),
            loss_generator=metrics.get('loss_generator', 0.0),
            loss_discriminator=metrics.get('loss_discriminator', 0.0),
        )
    
    # Run training with callback
    train(args, metric_callback=metric_callback)


def get_search_space() -> Dict[str, Any]:
    """
    Define the hyperparameter search space.
    
    Returns:
        Dictionary of hyperparameters with their search ranges
    """
    return {
        # Learning rate (log uniform for wide range)
        'lr': tune.loguniform(1e-5, 1e-3),
        
        # Loss weights
        'lambda_pixel': tune.uniform(0.5, 2.0),
        'lambda_perceptual': tune.uniform(0.01, 0.5),
        'lambda_edge': tune.uniform(0.01, 0.3),
        'lambda_freq': tune.uniform(0.01, 0.2),
        'lambda_adv': tune.uniform(0.001, 0.1),
        
        # Model architecture (discrete choices)
        'g_channels': tune.choice([64, 96, 128]),
        'g_blocks': tune.choice([12, 16, 20, 24]),
        'growth_channels': tune.choice([24, 32, 48]),
        'd_channels': tune.choice([48, 64, 96]),
        
        # Batch size affects training dynamics
        'batch_size': tune.choice([4, 8, 12, 16]),
    }


def get_minimal_search_space() -> Dict[str, Any]:
    """
    Minimal search space for quick experiments (fewer hyperparameters).
    
    Returns:
        Dictionary of key hyperparameters
    """
    return {
        'lr': tune.loguniform(1e-5, 1e-3),
        'lambda_perceptual': tune.uniform(0.05, 0.3),
        'lambda_edge': tune.uniform(0.05, 0.2),
        'lambda_adv': tune.uniform(0.005, 0.05),
    }


def get_architecture_search_space() -> Dict[str, Any]:
    """
    Search space focused on model architecture.
    
    Returns:
        Dictionary of architecture hyperparameters
    """
    return {
        'lr': tune.loguniform(5e-5, 5e-4),
        'g_channels': tune.choice([64, 96, 128, 160]),
        'g_blocks': tune.choice([12, 16, 20, 24, 28]),
        'growth_channels': tune.choice([24, 32, 48]),
        'd_channels': tune.choice([48, 64, 96, 128]),
        'batch_size': tune.choice([4, 8, 12]),
    }


def get_loss_weights_search_space() -> Dict[str, Any]:
    """
    Search space focused on loss weight tuning.
    
    Returns:
        Dictionary of loss weight hyperparameters
    """
    return {
        'lambda_pixel': tune.uniform(0.5, 2.0),
        'lambda_perceptual': tune.uniform(0.01, 0.5),
        'lambda_edge': tune.uniform(0.01, 0.3),
        'lambda_freq': tune.uniform(0.01, 0.2),
        'lambda_adv': tune.uniform(0.001, 0.1),
    }


def run_hyperparameter_search(
    base_args: Namespace,
    search_space: Dict[str, Any],
    num_samples: int = 20,
    max_concurrent_trials: int = 1,
    metric: str = 'loss_generator',
    mode: str = 'min',
    scheduler: Optional[str] = 'asha',
    search_alg: Optional[str] = 'optuna',
    grace_period: int = 1000,
    reduction_factor: int = 2,
    local_dir: str = './ray_results',
    name: str = 'sr_hparam_search',
    resume: bool = False,
) -> tune.ResultGrid:
    """
    Run hyperparameter search using Ray Tune.
    
    Args:
        base_args: Base training arguments
        search_space: Hyperparameter search space
        num_samples: Number of trials to run
        max_concurrent_trials: Max parallel trials (limited by GPU)
        metric: Metric to optimize
        mode: 'min' or 'max'
        scheduler: 'asha', 'pbt', or None
        search_alg: 'optuna', 'random', or None
        grace_period: Min steps before early stopping
        reduction_factor: ASHA reduction factor
        local_dir: Directory for Ray results
        name: Experiment name
        resume: Whether to resume from previous run
    
    Returns:
        ResultGrid with all trial results
    """
    # Initialize Ray if not already
    if not ray.is_initialized():
        ray.init(ignore_reinit_error=True)
    
    # Create scheduler for early stopping
    tune_scheduler = None
    if scheduler == 'asha':
        tune_scheduler = ASHAScheduler(
            time_attr='step',
            metric=metric,
            mode=mode,
            max_t=base_args.num_steps,
            grace_period=grace_period,
            reduction_factor=reduction_factor,
        )
    elif scheduler == 'pbt':
        tune_scheduler = PopulationBasedTraining(
            time_attr='step',
            metric=metric,
            mode=mode,
            perturbation_interval=5000,
            hyperparam_mutations={
                'lr': tune.loguniform(1e-5, 1e-3),
                'lambda_adv': tune.uniform(0.001, 0.1),
            },
        )
    
    # Create search algorithm
    tune_search_alg = None
    if search_alg == 'optuna':
        tune_search_alg = OptunaSearch(metric=metric, mode=mode)
    
    # Create trainable with base_args bound
    trainable = tune.with_parameters(train_wrapper, base_args=base_args)
    
    # Add resource requirements
    trainable = tune.with_resources(
        trainable,
        resources={'cpu': base_args.num_workers + 1, 'gpu': 1}
    )
    
    # Run tuning
    tuner = tune.Tuner(
        trainable,
        param_space=search_space,
        tune_config=tune.TuneConfig(
            num_samples=num_samples,
            max_concurrent_trials=max_concurrent_trials,
            scheduler=tune_scheduler,
            search_alg=tune_search_alg,
        ),
        run_config=RunConfig(
            name=name,
            storage_path=local_dir,
            stop={'step': base_args.num_steps},
            verbose=2,
        ),
    )
    
    if resume:
        tuner = tune.Tuner.restore(
            path=os.path.join(local_dir, name),
            trainable=trainable,
            resume_errored=True,
        )
    
    results = tuner.fit()
    
    return results


def print_best_results(results: tune.ResultGrid, top_k: int = 5) -> None:
    """
    Print the best trial results.
    
    Args:
        results: Ray Tune results
        top_k: Number of top results to print
    """
    print("\n" + "=" * 80)
    print("HYPERPARAMETER SEARCH RESULTS")
    print("=" * 80)
    
    # Get best result
    best_result = results.get_best_result(metric='loss_generator', mode='min')
    
    print("\nBest trial configuration:")
    print("-" * 40)
    for key, value in best_result.config.items():
        print(f"  {key}: {value}")
    
    print(f"\nBest trial final metrics:")
    print("-" * 40)
    for key, value in best_result.metrics.items():
        if isinstance(value, float):
            print(f"  {key}: {value:.6f}")
        else:
            print(f"  {key}: {value}")
    
    # Print top-k results
    print(f"\nTop {top_k} trials by loss_generator:")
    print("-" * 80)
    
    df = results.get_dataframe()
    if 'loss_generator' in df.columns:
        top_df = df.nsmallest(top_k, 'loss_generator')
        for idx, row in top_df.iterrows():
            print(f"\nTrial {idx}:")
            print(f"  loss_generator: {row.get('loss_generator', 'N/A'):.6f}")
            print(f"  loss_cycle: {row.get('loss_cycle', 'N/A'):.6f}")
            # Print config columns
            config_cols = [c for c in row.index if c.startswith('config/')]
            for col in config_cols:
                param_name = col.replace('config/', '')
                print(f"  {param_name}: {row[col]}")


def generate_train_command(config: Dict[str, Any], base_args: Namespace) -> str:
    """
    Generate a train.sh command with the best hyperparameters.
    
    Args:
        config: Best hyperparameter config
        base_args: Base arguments
    
    Returns:
        Command string for training with these hyperparameters
    """
    cmd_parts = ['python train_sr.py']
    
    # Add base args
    cmd_parts.append(f'--csv {base_args.csv}')
    if base_args.data_root:
        cmd_parts.append(f'--data-root {base_args.data_root}')
    else:
        cmd_parts.append(f'--grpc-address {base_args.grpc_address}')
    
    # Add tuned hyperparameters
    param_mapping = {
        'lr': '--lr',
        'lambda_pixel': '--lambda-pixel',
        'lambda_perceptual': '--lambda-perceptual',
        'lambda_edge': '--lambda-edge',
        'lambda_freq': '--lambda-freq',
        'lambda_adv': '--lambda-adv',
        'g_channels': '--g-channels',
        'g_blocks': '--g-blocks',
        'growth_channels': '--growth-channels',
        'd_channels': '--d-channels',
        'batch_size': '--batch-size',
    }
    
    for key, flag in param_mapping.items():
        if key in config:
            value = config[key]
            if isinstance(value, float):
                cmd_parts.append(f'{flag} {value:.6f}')
            else:
                cmd_parts.append(f'{flag} {value}')
    
    return ' \\\n    '.join(cmd_parts)


def main():
    parser = argparse.ArgumentParser(
        description='Hyperparameter search for pathology super-resolution',
        formatter_class=argparse.ArgumentDefaultsHelpFormatter,
    )
    
    # Data source (same as train_sr.py)
    parser.add_argument('--csv', type=str, default='slides.csv',
                        help='Path to CSV file with slide metadata')
    parser.add_argument('--data-root', type=str, default=None,
                        help='Path to TIF files (uses OpenSlide)')
    parser.add_argument('--grpc-address', type=str, default='localhost:8080',
                        help='gRPC server address')
    parser.add_argument('--train-level', type=int, default=0,
                        help='Mip level for training')
    
    # Training settings (passed to train_sr.py)
    parser.add_argument('--num-steps', type=int, default=10000,
                        help='Training steps per trial (shorter for search)')
    parser.add_argument('--pretrain-steps', type=int, default=200,
                        help='Pretrain steps without adversarial loss')
    parser.add_argument('--num-workers', type=int, default=4,
                        help='DataLoader workers')
    parser.add_argument('--device', type=str, default='cuda',
                        help='Training device')
    parser.add_argument('--log-interval', type=int, default=100,
                        help='Logging interval (also Ray Tune reporting)')
    
    # Search settings
    parser.add_argument('--num-samples', type=int, default=20,
                        help='Number of trials to run')
    parser.add_argument('--max-concurrent', type=int, default=1,
                        help='Max concurrent trials (limited by GPU)')
    parser.add_argument('--search-space', type=str, default='full',
                        choices=['full', 'minimal', 'architecture', 'loss_weights'],
                        help='Which search space to use')
    parser.add_argument('--scheduler', type=str, default='asha',
                        choices=['asha', 'pbt', 'none'],
                        help='Trial scheduler for early stopping')
    parser.add_argument('--search-alg', type=str, default='optuna',
                        choices=['optuna', 'random'],
                        help='Search algorithm')
    parser.add_argument('--grace-period', type=int, default=1000,
                        help='Min steps before early stopping')
    parser.add_argument('--metric', type=str, default='loss_generator',
                        choices=['loss_generator', 'loss_cycle', 'loss_perceptual', 'loss_edge'],
                        help='Metric to optimize')
    
    # Output settings
    parser.add_argument('--ray-dir', type=str, default='./ray_results',
                        help='Directory for Ray Tune results')
    parser.add_argument('--experiment-name', type=str, default='sr_hparam_search',
                        help='Experiment name')
    parser.add_argument('--out-dir', type=str, default='./checkpoints_hparam',
                        help='Checkpoint directory for trials')
    parser.add_argument('--log-dir', type=str, default='./runs_hparam',
                        help='TensorBoard log directory for trials')
    parser.add_argument('--resume', action='store_true',
                        help='Resume from previous run')
    
    args = parser.parse_args()
    
    # Create base args namespace for train_sr.py
    base_args = Namespace(
        csv=args.csv,
        data_root=args.data_root,
        grpc_address=args.grpc_address,
        train_level=args.train_level,
        out_dir=args.out_dir,
        log_dir=args.log_dir,
        num_steps=args.num_steps,
        pretrain_steps=args.pretrain_steps,
        num_workers=args.num_workers,
        device=args.device,
        log_interval=args.log_interval,
        # Defaults (will be overridden by search)
        batch_size=8,
        lr=1e-4,
        lambda_pixel=1.0,
        lambda_perceptual=0.1,
        lambda_edge=0.1,
        lambda_freq=0.05,
        lambda_adv=0.01,
        g_channels=128,
        g_blocks=20,
        growth_channels=32,
        d_channels=64,
        use_rrdb=True,
        color_jitter=False,
        color_jitter_strength=0.05,
        max_vram_gb=27.5,
        sample_interval=2000,
        save_interval=5000,
        log_images_interval=1000,
        run_name=None,
    )
    
    # Select search space
    search_spaces = {
        'full': get_search_space,
        'minimal': get_minimal_search_space,
        'architecture': get_architecture_search_space,
        'loss_weights': get_loss_weights_search_space,
    }
    search_space = search_spaces[args.search_space]()
    
    print("=" * 80)
    print("PATHOLOGY SUPER-RESOLUTION HYPERPARAMETER SEARCH")
    print("=" * 80)
    print(f"\nSearch space: {args.search_space}")
    print(f"Parameters to tune: {list(search_space.keys())}")
    print(f"Number of trials: {args.num_samples}")
    print(f"Steps per trial: {args.num_steps}")
    print(f"Scheduler: {args.scheduler}")
    print(f"Search algorithm: {args.search_alg}")
    print(f"Metric to optimize: {args.metric} (minimize)")
    print("=" * 80)
    
    # Run search
    scheduler = None if args.scheduler == 'none' else args.scheduler
    search_alg = None if args.search_alg == 'random' else args.search_alg
    
    results = run_hyperparameter_search(
        base_args=base_args,
        search_space=search_space,
        num_samples=args.num_samples,
        max_concurrent_trials=args.max_concurrent,
        metric=args.metric,
        mode='min',
        scheduler=scheduler,
        search_alg=search_alg,
        grace_period=args.grace_period,
        local_dir=args.ray_dir,
        name=args.experiment_name,
        resume=args.resume,
    )
    
    # Print results
    print_best_results(results)
    
    # Generate training command for best config
    best_result = results.get_best_result(metric='loss_generator', mode='min')
    print("\n" + "=" * 80)
    print("RECOMMENDED TRAINING COMMAND")
    print("=" * 80)
    print("\n" + generate_train_command(best_result.config, base_args))
    print()


if __name__ == '__main__':
    main()
