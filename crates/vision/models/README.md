# Bundled vision models

Weights come from [OpenCV Zoo commit `48b10fa`](https://github.com/opencv/opencv_zoo/tree/48b10fa505aeb51fbd127fc60a5c660040e823b7/models). fishing embeds the weights in its executable. Model inference takes place locally with ONNX Runtime.

Run `./packaging/Prepare-Assets.ps1` from the repository root to fetch and verify the two weight files before compiling.

| Model | License | SHA-256 |
| --- | --- | --- |
| `face_detection_yunet_2026may.onnx` | [MIT](YuNet-LICENSE) | `ebafce4e3c118d6554634be5c27ab333b4c047a9a8c3faf1d7cf93101c22f0f0` |
| `face_recognition_sface_2021dec.onnx` | [Apache-2.0](SFace-LICENSE) | `0ba9fbfa01b5270c96627c4ef784da859931e02f04419c829e83484087c34e79` |

The [YuNet release](https://github.com/opencv/opencv_zoo/tree/48b10fa505aeb51fbd127fc60a5c660040e823b7/models/face_detection_yunet) supports dynamic dimensions. The [SFace release](https://github.com/opencv/opencv_zoo/tree/48b10fa505aeb51fbd127fc60a5c660040e823b7/models/face_recognition_sface) takes 112 × 112 aligned RGB input and returns a 128-element embedding. `EMBEDDING_MODEL` identifies the exact recognition model and preprocessing version for saved templates. Change it when weights or preprocessing change in a way that affects matching.
