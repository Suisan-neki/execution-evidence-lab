"""Synthetic CPU inference. This is a numerical plumbing check, not a clinical model."""
import hashlib
import json
from pathlib import Path
import numpy as np
import torch
import monai
from monai.data import create_test_image_2d
from monai.inferers import sliding_window_inference

# Defined before the trial: compare every float32 pixel to 2*x + 0.25.
ATOL, RTOL = 1e-5, 1e-5
torch.set_num_threads(1)
torch.use_deterministic_algorithms(True)
image, labels = create_test_image_2d(64, 64, num_objs=4, rad_min=3, rad_max=10,
                                    random_state=np.random.RandomState(144))
x = torch.from_numpy(image.astype(np.float32))[None, None]
model = torch.nn.Conv2d(1, 1, kernel_size=1)
with torch.no_grad():
    model.weight.fill_(2.0)
    model.bias.fill_(0.25)
model.eval()
out = Path("/result")
torch.save(model.state_dict(), out / "checkpoint.pt")
loaded = torch.nn.Conv2d(1, 1, kernel_size=1)
loaded.load_state_dict(torch.load(out / "checkpoint.pt", map_location="cpu", weights_only=True))
loaded.eval()
with torch.inference_mode():
    prediction = sliding_window_inference(x, (32, 32), 2, loaded, overlap=0.25, mode="constant", device="cpu", sw_device="cpu")
    expected = x * 2 + 0.25
torch.testing.assert_close(prediction, expected, atol=ATOL, rtol=RTOL)
if prediction.shape != (1, 1, 64, 64) or not torch.isfinite(prediction).all():
    raise RuntimeError("invalid inference output")
np.save(out / "input.npy", x.numpy(), allow_pickle=False)
np.save(out / "labels.npy", labels, allow_pickle=False)
np.save(out / "prediction.npy", prediction.numpy(), allow_pickle=False)
saved = np.load(out / "prediction.npy", allow_pickle=False)
np.testing.assert_allclose(saved, expected.numpy(), atol=ATOL, rtol=RTOL)
files = {name: hashlib.sha256((out / name).read_bytes()).hexdigest()
         for name in ["input.npy", "labels.npy", "checkpoint.pt", "prediction.npy"]}
summary = dict(device="cpu", torch=torch.__version__, numpy=np.__version__, monai=monai.__version__,
               seed=144, shape=list(prediction.shape), atol=ATOL, rtol=RTOL,
               reference="Every output pixel equals 2 * input + 0.25 within atol + rtol * abs(reference)",
               max_abs_error=float((prediction - expected).abs().max()), files_sha256=files,
               limit="Synthetic affine predictor through MONAI sliding-window inference; no training or clinical performance evaluation.")
(out / "inference.json").write_text(json.dumps(summary, indent=2))
print(json.dumps(summary))
