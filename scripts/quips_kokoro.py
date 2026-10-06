"""Renders Rex's quip lines with Kokoro-82M (voice am_onyx) for scripts/gen-quips.sh.

Reads `id<TAB>text` lines on stdin and writes <outdir>/<id>.wav (24 kHz mono). The model
revision is pinned and the weights and voice are checked by sha256, so a run never silently
picks up different files.
"""

import hashlib
import os
import sys

REPO = "hexgrad/Kokoro-82M"
REVISION = "f3ff3571791e39611d31c381e3a41a3af07b4987"
VOICE = "am_onyx"
FILES = {
    "kokoro-v1_0.pth": "496dba118d1a58f5f3db2efc88dbdc216e0483fc89fe6e47ee1f2c53f18ad1e4",
    f"voices/{VOICE}.pt": "e8452be16cd0f6da7b4579eaf7b1e4506e92524882053d86d72b96b9a7fed584",
}


def use_system_espeak():
    """The espeakng-loader wheel ships a broken data path; prefer the system espeak-ng."""
    lib = os.environ.get("ESPEAK_LIB", "/usr/lib/x86_64-linux-gnu/libespeak-ng.so.1")
    data = os.environ.get("ESPEAK_DATA", "/usr/lib/x86_64-linux-gnu/espeak-ng-data")
    if os.path.exists(lib) and os.path.isdir(data):
        import espeakng_loader

        espeakng_loader.get_library_path = lambda: lib
        espeakng_loader.get_data_path = lambda: data


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    if len(sys.argv) not in (2, 3):
        sys.exit("usage: quips_kokoro.py OUTDIR [SPEED] < id<TAB>text lines")
    outdir = sys.argv[1]
    speed = float(sys.argv[2]) if len(sys.argv) == 3 else 0.95

    use_system_espeak()
    from huggingface_hub import snapshot_download
    from kokoro import KModel, KPipeline
    import soundfile as sf
    import torch

    root = snapshot_download(REPO, revision=REVISION, allow_patterns=["config.json", *FILES])
    for rel, want in FILES.items():
        got = sha256(os.path.join(root, rel))
        if got != want:
            sys.exit(f"quips_kokoro: {rel} sha256 {got} does not match the pinned {want}")

    model = KModel(
        repo_id=REPO,
        config=os.path.join(root, "config.json"),
        model=os.path.join(root, "kokoro-v1_0.pth"),
    ).eval()
    pipe = KPipeline(lang_code="a", repo_id=REPO, model=model)
    voice = os.path.join(root, "voices", f"{VOICE}.pt")

    for line in sys.stdin:
        line = line.rstrip("\n")
        if not line:
            continue
        qid, text = line.split("\t", 1)
        chunks = [audio for _, _, audio in pipe(text, voice=voice, speed=speed)]
        if not chunks:
            sys.exit(f"quips_kokoro: no audio for {qid}")
        sf.write(os.path.join(outdir, f"{qid}.wav"), torch.cat(chunks).numpy(), 24000)


if __name__ == "__main__":
    main()
