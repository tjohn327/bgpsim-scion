#!/bin/bash

# Run Large Topology Evaluation and Scalability Benchmarks

set -e

echo "====================================="
echo "SCION Evaluation Suite"
echo "====================================="
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo "Error: cargo not found. Please ensure Rust is installed."
    exit 1
fi

# Create output directory
RESULTS_DIR="$(dirname "$0")/results"
mkdir -p "$RESULTS_DIR"

echo "Results will be saved to: $RESULTS_DIR"
echo ""

# Run large topology example
echo "====================================="
echo "1. Running Large Multi-ISD Topology Example"
echo "====================================="
echo ""

cargo run --release --example large_multi_isd --features scion 2>&1 | tee "$RESULTS_DIR/large_topology_output.txt"

echo ""
echo "====================================="
echo "2. Running Scalability Benchmarks"
echo "====================================="
echo ""

# Run scalability benchmarks
cargo bench --bench scion_scalability --features scion -- --save-baseline scion_scalability 2>&1 | tee "$RESULTS_DIR/scalability_bench.txt"

echo ""
echo "====================================="
echo "3. Results Summary"
echo "====================================="
echo ""

# Extract key metrics
echo "Extracting key metrics..."

# From large topology run
if [ -f "$RESULTS_DIR/large_topology_output.txt" ]; then
    echo "Large Topology Results:" > "$RESULTS_DIR/summary.txt"
    echo "======================" >> "$RESULTS_DIR/summary.txt"
    echo "" >> "$RESULTS_DIR/summary.txt"

    grep -A 20 "BGP vs SCION Comparison" "$RESULTS_DIR/large_topology_output.txt" >> "$RESULTS_DIR/summary.txt" || true
    echo "" >> "$RESULTS_DIR/summary.txt"
fi

# From benchmarks
if [ -f "$RESULTS_DIR/scalability_bench.txt" ]; then
    echo "Scalability Benchmark Results:" >> "$RESULTS_DIR/summary.txt"
    echo "=============================" >> "$RESULTS_DIR/summary.txt"
    echo "" >> "$RESULTS_DIR/summary.txt"

    grep "time:" "$RESULTS_DIR/scalability_bench.txt" >> "$RESULTS_DIR/summary.txt" || true
fi

cat "$RESULTS_DIR/summary.txt"

echo ""
echo "====================================="
echo "Evaluation Complete!"
echo "====================================="
echo ""
echo "Results saved to: $RESULTS_DIR"
echo ""
echo "Next steps:"
echo "  1. Review results in $RESULTS_DIR"
echo "  2. Run visualization: python3 examples/large_topology/visualize.py"
echo "  3. Generate plots: python3 examples/large_topology/plot_scalability.py"
echo ""
