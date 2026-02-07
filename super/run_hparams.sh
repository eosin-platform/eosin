#!/bin/bash
#
# Grid Search for Pathology Super-Resolution Hyperparameters
#
# Runs training for 500 steps per configuration to quickly evaluate
# different hyperparameter combinations.
#
# Usage:
#   ./run_hparams.sh                    # Uses default gRPC
#   ./run_hparams.sh --data-root /path  # Uses local TIF files
#
set -euo pipefail
cd "$(dirname "$0")"
source .venv/bin/activate

# Configuration
CSV="${CSV:-slides.csv}"
NUM_STEPS=500
PRETRAIN_STEPS=100
LOG_INTERVAL=50
BATCH_SIZE=8
NUM_WORKERS=4
DEVICE="cuda"

# Output directories
CHECKPOINT_DIR="./checkpoints_grid"
LOG_DIR="./runs_grid"
RESULTS_FILE="./grid_search_results.csv"

# Parse optional data source argument
#DATA_SOURCE_ARGS="--grpc-address localhost:8080"
DATA_SOURCE_ARGS="--data-root $HOME/Repositories/camelyon/CAMELYON17/images"
#if [[ "$1" == "--data-root" ]] && [[ -n "$2" ]]; then
#    DATA_SOURCE_ARGS="--data-root $2"
#    shift 2
#fi

# Create output directories
mkdir -p "$CHECKPOINT_DIR"
mkdir -p "$LOG_DIR"

# Initialize results file with header
echo "run_name,lr,lambda_pixel,lambda_perceptual,lambda_edge,lambda_freq,lambda_adv,g_channels,g_blocks,final_loss_generator" > "$RESULTS_FILE"

# =============================================================================
# Hyperparameter Grid (focused on most impactful parameters)
# =============================================================================

# Learning rates (log scale around default 1e-4)
LR_VALUES=(5e-5 1e-4 2e-4)

# Loss weights - search around defaults
#LAMBDA_PIXEL_VALUES=(0.5 1.0 2.0)
#LAMBDA_PERCEPTUAL_VALUES=(0.05 0.1 0.2)
#LAMBDA_EDGE_VALUES=(0.05 0.1 0.2)
#LAMBDA_FREQ_VALUES=(0.02 0.05 0.1)
#LAMBDA_ADV_VALUES=(0.005 0.01 0.02)
LAMBDA_PIXEL_VALUES=(1.0)
LAMBDA_PERCEPTUAL_VALUES=(0.01)
LAMBDA_EDGE_VALUES=(0.1)
LAMBDA_FREQ_VALUES=(0.05)
LAMBDA_ADV_VALUES=(0.01)

# Architecture (keep fixed for speed, uncomment to search)
G_CHANNELS=128
G_BLOCKS=20

# =============================================================================
# Quick Grid Search (loss weights only, ~27 combinations)
# =============================================================================

run_quick_grid() {
    echo "=========================================="
    echo "Running Quick Grid Search (Loss Weights)"
    echo "=========================================="
    
    local run_count=0
    local total_runs=$(( ${#LR_VALUES[@]} * ${#LAMBDA_PERCEPTUAL_VALUES[@]} * ${#LAMBDA_ADV_VALUES[@]} ))
    
    for lr in "${LR_VALUES[@]}"; do
        for lambda_perceptual in "${LAMBDA_PERCEPTUAL_VALUES[@]}"; do
            for lambda_adv in "${LAMBDA_ADV_VALUES[@]}"; do
                run_count=$((run_count + 1))
                
                local run_name="grid_lr${lr}_perc${lambda_perceptual}_adv${lambda_adv}"
                
                echo ""
                echo "[$run_count/$total_runs] Running: $run_name"
                echo "  lr=$lr, lambda_perceptual=$lambda_perceptual, lambda_adv=$lambda_adv"
                echo ""
                
                python train_sr.py \
                    --csv "$CSV" \
                    $DATA_SOURCE_ARGS \
                    --num-steps $NUM_STEPS \
                    --pretrain-steps $PRETRAIN_STEPS \
                    --batch-size $BATCH_SIZE \
                    --num-workers $NUM_WORKERS \
                    --device $DEVICE \
                    --lr $lr \
                    --lambda-pixel 1.0 \
                    --lambda-perceptual $lambda_perceptual \
                    --lambda-edge 0.1 \
                    --lambda-freq 0.05 \
                    --lambda-adv $lambda_adv \
                    --g-channels $G_CHANNELS \
                    --g-blocks $G_BLOCKS \
                    --out-dir "$CHECKPOINT_DIR" \
                    --log-dir "$LOG_DIR" \
                    --run-name "$run_name" \
                    --log-interval $LOG_INTERVAL \
                    --sample-interval 1000 \
                    --save-interval 1000 \
                    2>&1 | tee "$LOG_DIR/${run_name}.log"
                
                # Extract final loss from log (last line with L_cycle)
                local final_loss=$(grep -E "Step [0-9]+/${NUM_STEPS}" "$LOG_DIR/${run_name}.log" | tail -1 | grep -oP 'L_cycle: \K[0-9.]+' || echo "N/A")
                
                # Record result
                echo "$run_name,$lr,1.0,$lambda_perceptual,0.1,0.05,$lambda_adv,$G_CHANNELS,$G_BLOCKS,$final_loss" >> "$RESULTS_FILE"
            done
        done
    done
}

# =============================================================================
# Full Grid Search (all loss weights, ~243 combinations - takes longer)
# =============================================================================

run_full_grid() {
    echo "=========================================="
    echo "Running Full Grid Search (All Loss Weights)"
    echo "=========================================="
    
    local run_count=0
    local total_runs=$(( ${#LR_VALUES[@]} * ${#LAMBDA_PIXEL_VALUES[@]} * ${#LAMBDA_PERCEPTUAL_VALUES[@]} * ${#LAMBDA_EDGE_VALUES[@]} * ${#LAMBDA_ADV_VALUES[@]} ))
    
    for lr in "${LR_VALUES[@]}"; do
        for lambda_pixel in "${LAMBDA_PIXEL_VALUES[@]}"; do
            for lambda_perceptual in "${LAMBDA_PERCEPTUAL_VALUES[@]}"; do
                for lambda_edge in "${LAMBDA_EDGE_VALUES[@]}"; do
                    for lambda_adv in "${LAMBDA_ADV_VALUES[@]}"; do
                        run_count=$((run_count + 1))
                        
                        local run_name="grid_lr${lr}_pix${lambda_pixel}_perc${lambda_perceptual}_edge${lambda_edge}_adv${lambda_adv}"
                        
                        echo ""
                        echo "[$run_count/$total_runs] Running: $run_name"
                        echo ""
                        
                        python train_sr.py \
                            --csv "$CSV" \
                            $DATA_SOURCE_ARGS \
                            --num-steps $NUM_STEPS \
                            --pretrain-steps $PRETRAIN_STEPS \
                            --batch-size $BATCH_SIZE \
                            --num-workers $NUM_WORKERS \
                            --device $DEVICE \
                            --lr $lr \
                            --lambda-pixel $lambda_pixel \
                            --lambda-perceptual $lambda_perceptual \
                            --lambda-edge $lambda_edge \
                            --lambda-freq 0.05 \
                            --lambda-adv $lambda_adv \
                            --g-channels $G_CHANNELS \
                            --g-blocks $G_BLOCKS \
                            --out-dir "$CHECKPOINT_DIR" \
                            --log-dir "$LOG_DIR" \
                            --run-name "$run_name" \
                            --log-interval $LOG_INTERVAL \
                            --sample-interval 1000 \
                            --save-interval 1000 \
                            2>&1 | tee "$LOG_DIR/${run_name}.log"
                        
                        local final_loss=$(grep -E "Step [0-9]+/${NUM_STEPS}" "$LOG_DIR/${run_name}.log" | tail -1 | grep -oP 'L_cycle: \K[0-9.]+' || echo "N/A")
                        echo "$run_name,$lr,$lambda_pixel,$lambda_perceptual,$lambda_edge,0.05,$lambda_adv,$G_CHANNELS,$G_BLOCKS,$final_loss" >> "$RESULTS_FILE"
                    done
                done
            done
        done
    done
}

# =============================================================================
# Focused Grid Search (key hyperparameters, ~27 combinations)
# =============================================================================

run_focused_grid() {
    echo "=========================================="
    echo "Running Focused Grid Search"
    echo "=========================================="
    echo "Searching: lr, lambda_perceptual, lambda_edge, lambda_adv"
    
    local run_count=0
    local total_runs=$(( ${#LR_VALUES[@]} * ${#LAMBDA_PERCEPTUAL_VALUES[@]} * ${#LAMBDA_EDGE_VALUES[@]} ))
    
    for lr in "${LR_VALUES[@]}"; do
        for lambda_perceptual in "${LAMBDA_PERCEPTUAL_VALUES[@]}"; do
            for lambda_edge in "${LAMBDA_EDGE_VALUES[@]}"; do
                run_count=$((run_count + 1))
                
                local run_name="grid_lr${lr}_perc${lambda_perceptual}_edge${lambda_edge}"
                
                echo ""
                echo "[$run_count/$total_runs] Running: $run_name"
                echo "  lr=$lr, lambda_perceptual=$lambda_perceptual, lambda_edge=$lambda_edge"
                echo ""
                
                python train_sr.py \
                    --csv "$CSV" \
                    $DATA_SOURCE_ARGS \
                    --num-steps $NUM_STEPS \
                    --pretrain-steps $PRETRAIN_STEPS \
                    --batch-size $BATCH_SIZE \
                    --num-workers $NUM_WORKERS \
                    --device $DEVICE \
                    --lr $lr \
                    --lambda-pixel 1.0 \
                    --lambda-perceptual $lambda_perceptual \
                    --lambda-edge $lambda_edge \
                    --lambda-freq 0.05 \
                    --lambda-adv 0.01 \
                    --g-channels $G_CHANNELS \
                    --g-blocks $G_BLOCKS \
                    --out-dir "$CHECKPOINT_DIR" \
                    --log-dir "$LOG_DIR" \
                    --run-name "$run_name" \
                    --log-interval $LOG_INTERVAL \
                    --sample-interval 1000 \
                    --save-interval 1000 \
                    2>&1 | tee "$LOG_DIR/${run_name}.log"
                
                local final_loss=$(grep -E "Step [0-9]+/${NUM_STEPS}" "$LOG_DIR/${run_name}.log" | tail -1 | grep -oP 'L_cycle: \K[0-9.]+' || echo "N/A")
                echo "$run_name,$lr,1.0,$lambda_perceptual,$lambda_edge,0.05,0.01,$G_CHANNELS,$G_BLOCKS,$final_loss" >> "$RESULTS_FILE"
            done
        done
    done
}

# =============================================================================
# Print Results Summary
# =============================================================================

print_summary() {
    echo ""
    echo "=========================================="
    echo "GRID SEARCH COMPLETE"
    echo "=========================================="
    echo ""
    echo "Results saved to: $RESULTS_FILE"
    echo ""
    echo "Top 5 configurations by loss:"
    echo "------------------------------"
    
    # Sort by final loss (skip header, handle N/A)
    tail -n +2 "$RESULTS_FILE" | \
        grep -v "N/A" | \
        sort -t',' -k10 -n | \
        head -5 | \
        while IFS=',' read -r name lr pix perc edge freq adv gch gblk loss; do
            echo "  $name"
            echo "    lr=$lr, perceptual=$perc, edge=$edge, adv=$adv"
            echo "    final_loss=$loss"
            echo ""
        done
    
    echo ""
    echo "TensorBoard logs: $LOG_DIR"
    echo "View with: tensorboard --logdir $LOG_DIR"
}

# =============================================================================
# Main
# =============================================================================

echo "=========================================="
echo "Pathology Super-Resolution Grid Search"
echo "=========================================="
echo ""
echo "Configuration:"
echo "  CSV: $CSV"
echo "  Steps per run: $NUM_STEPS"
echo "  Pretrain steps: $PRETRAIN_STEPS"
echo "  Batch size: $BATCH_SIZE"
echo "  Data source: $DATA_SOURCE_ARGS"
echo ""

# Select search mode
MODE="${GRID_MODE:-focused}"

case "$MODE" in
    quick)
        run_quick_grid
        ;;
    full)
        run_full_grid
        ;;
    focused|*)
        run_focused_grid
        ;;
esac

print_summary
