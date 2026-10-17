"""A fresh process reads the saved result; this does not assess model accuracy."""
import hashlib
import json
from pathlib import Path
import numpy as np

root = Path("/result")
prediction = np.load(root / "prediction.npy", allow_pickle=False)
reference = np.load(root / "reference.npy", allow_pickle=False)
if prediction.shape != (1, 1, 64, 64) or prediction.dtype != np.float32 or not np.isfinite(prediction).all():
    raise ValueError("saved result shape, dtype or finiteness differs")
np.testing.assert_allclose(prediction, reference, atol=1e-6, rtol=1e-5)
report = dict(shape=list(prediction.shape), dtype=str(prediction.dtype), atol=1e-6, rtol=1e-5,
              max_abs_error=float(np.max(np.abs(prediction - reference))),
              file_sha256=hashlib.sha256((root / "prediction.npy").read_bytes()).hexdigest())
with (root / "readback.json").open("x") as output:
    json.dump(report, output, indent=2)
print(json.dumps(report))
