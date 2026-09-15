"""Small protocol shared by the installer and generation worker."""
import json
import sys


def emit(kind, **fields):
    print(json.dumps({"protocol": 1, "type": kind, **fields}), flush=True)


def progress(stage, percent, message):
    emit("progress", stage=stage, progress=percent, message=message)
