#!/bin/bash
# Wrapper script for pdftotext to extract PDF text to stdout
# Usage: pdftotext.sh <input.pdf>
#
# This wrapper is needed because contextd appends the file path as the
# last argument, but pdftotext needs the output file ("-" for stdout)
# to come after the input file.

if [ -z "$1" ]; then
    echo "Usage: $0 <input.pdf>" >&2
    exit 1
fi

pdftotext -layout "$1" -
