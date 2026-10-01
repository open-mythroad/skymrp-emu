#!/bin/sh
set -e

cd "$(dirname "$0")"

PHASE="$1"
if [ "$PHASE" = "--prepare-files" ]; then
    shift

    if [ "$#" -ne 0 ]; then
        echo "Error! Unexpected argument"
        exit 1
    fi

    rm -rf new_release
    mkdir new_release

    cp ../README.md new_release/README.txt
    cp ../README_CN.md new_release/README_CN.txt
    cp ../CHANGELOG.md new_release/CHANGELOG.txt
    cp gpl-3.0.txt new_release/COPYING
elif [ "$PHASE" = "--create-zip" ]; then
    shift

    PATH_TO_BINARY="$1"
    if [ -z "$PATH_TO_BINARY" ]; then
        echo "Error! Path to binary must be provided"
        exit 1
    fi
    shift

    if [ "$1" != "-o" ]; then
        echo "Error! -o expected"
        exit 1
    fi
    shift

    OUTPUT_PATH="$1"
    if [ -z "$OUTPUT_PATH" ]; then
        echo "Error! Output path must be provided"
        exit 1
    fi
    shift

    if [ "$#" -ne 0 ]; then
        echo "Error! Unexpected argument"
        exit 1
    fi

    if [ ! -f "../$PATH_TO_BINARY" ]; then
        echo "Error! Binary not found: $PATH_TO_BINARY"
        exit 1
    fi

    if [ ! -d "new_release" ]; then
        echo "Error! --prepare-files phase must be run first"
        exit 1
    fi

    OUTPUT_DIR=$(dirname "$OUTPUT_PATH")
    OUTPUT_NAME=$(basename "$OUTPUT_PATH")
    OUTPUT_DIR=$(cd "../$OUTPUT_DIR" && pwd)
    OUTPUT_PATH="$OUTPUT_DIR/$OUTPUT_NAME"
    rm -f "$OUTPUT_PATH"

    zip -j "$OUTPUT_PATH" "../$PATH_TO_BINARY"
    cd new_release/
    zip -r "$OUTPUT_PATH" *
else
    echo "Unknown or missing phase."
    echo
    echo "Usage, phase 1:"
    echo
    echo "  ./prepare-release.sh --prepare-files"
    echo
    echo "Usage, phase 2:"
    echo
    echo "  ./prepare-release.sh --create-zip path/to/skymrp -o skymrp_vX.Y.Z_macOS_aarch64.zip"
    exit 1
fi
