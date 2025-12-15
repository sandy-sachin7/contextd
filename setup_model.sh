#!/bin/bash
set -e

MODEL_DIR="models"
mkdir -p "$MODEL_DIR"

echo "Downloading model and tokenizer..."

# all-MiniLM-L6-v2
# We need model.onnx and tokenizer.json
# Using a quantized version or standard? Standard is small enough (~90MB).

if [ ! -f "$MODEL_DIR/model.onnx" ]; then
    echo "Fetching model.onnx..."
    curl -L -o "$MODEL_DIR/model.onnx" "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/model.onnx"
fi

if [ ! -f "$MODEL_DIR/tokenizer.json" ]; then
    echo "Fetching tokenizer.json..."
    curl -L -o "$MODEL_DIR/tokenizer.json" "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/tokenizer.json"
fi

if [ ! -f "$MODEL_DIR/vocab.txt" ]; then
    echo "Fetching vocab.txt..."
    curl -L -o "$MODEL_DIR/vocab.txt" "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/vocab.txt"
fi

if [ ! -f "$MODEL_DIR/tokenizer_config.json" ]; then
    echo "Fetching tokenizer_config.json..."
    curl -L -o "$MODEL_DIR/tokenizer_config.json" "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/tokenizer_config.json"
fi

if [ ! -f "$MODEL_DIR/special_tokens_map.json" ]; then
    echo "Fetching special_tokens_map.json..."
    curl -L -o "$MODEL_DIR/special_tokens_map.json" "https://huggingface.co/optimum/all-MiniLM-L6-v2/resolve/main/special_tokens_map.json"
fi

echo "Model setup complete in $MODEL_DIR"
