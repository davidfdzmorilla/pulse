# Drum model

The network behind «Que lo toque la IA» in Generar ritmo
(`crates/stems/src/drums.rs`, `crates/stems/models/drums.onnx`).

A causal transformer (4.8M parameters) over sixteenth steps: each step is the
level (silent, ghost, normal, accented) of eight sounds, and the model gives the
next step from up to eight bars before it and the place in an eight-bar phrase.

    cd ml/drums
    mkdir -p data && cd data
    curl -LO https://storage.googleapis.com/magentadata/datasets/groove/groove-v1.0.0-midionly.zip && unzip -q groove-v1.0.0-midionly.zip
    curl -L http://hog.ee.columbia.edu/craffel/lmd/lmd_full.tar.gz | tar xz
    cd ..
    uv run python extract.py data/groove --out data/groove.npz
    uv run python extract.py data/lmd_full --out data/lmd.npz          # ~10 min
    uv run python train.py data/lmd.npz data/groove.npz --out runs/drums  # ~2 h on an M1 Max
    uv run python look.py runs/drums/model.pt --style techno              # what it plays
    uv run python train.py --export runs/drums/model.pt --onnx ../../crates/stems/models/drums.onnx

Both datasets are CC BY 4.0; see `crates/stems/models/DRUMS_NOTICE`.
