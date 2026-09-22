# SoundRadar

游戏音效实时识别助手（Windows）。听扬声器回环，对照本地音效库，在命中时提示“这是哪个音效”。

本仓库是原 Go 版 `soundradar 0.6.x` 的 **Rust 原生性能重写**：CLI / `library.srz` / `config.json` / `index.bin` 格式兼容，热路径（帧 log-mel、滑窗指纹、i8 检索）显著加快。

## 功能

- `app`（双击默认）：管理 UI + 持续实时识别 + 覆盖层
- `serve`：音效库管理网页 + JSON API（仅 127.0.0.1）
- `match`：用 wav/mp3 识别音效
- `index rebuild`：构建 mel-goertzel-v1 指纹索引（SRZ1）
- `live` / `overlay` / `recall` / `capture` / `devices` / `version`

## 算法

| 项 | 值 |
|----|----|
| 特征 | mel-goertzel-v1 |
| 维数 | 2048 = 64 Mel × 32 帧 |
| 帧 | 48 kHz / 1024 / hop 256（5.333 ms）/ Hann 周期窗 / 去直流 |
| 归一化 | mean-subtract + L2（余弦 = 点积） |
| 存储 | i8 量化，SRZ1 索引 |

## 构建

```powershell
cargo build --release
# 产物 target\release\soundradar.exe
```

依赖：Rust 1.70+，Windows 10/11 x64。无 CGO。

## 使用

```text
soundradar.exe                 # 双击 / 无参数 → 完整应用
soundradar.exe match --wav a.wav
soundradar.exe serve --open
soundradar.exe index rebuild
soundradar.exe version
```

详细步骤见发行包内 `使用说明.txt`。

## 性能（相对 0.6.x Go 版，9 条 3s 样本）

| 路径 | Go | Rust |
|------|----|------|
| match 检索 | 55–78 ms | 5–11 ms |
| match 进程墙钟 | 70–150 ms | 21–29 ms |
| index rebuild | ~294 ms | ~116 ms |
| 二进制 | 13.5 MB | 3.0 MB |

## 目录

```text
src/main.rs        CLI 入口（无参数 = app）
src/dsp.rs         mel / FFT / 指纹
src/index.rs       SRZ1 索引与检索
src/library.rs     library.srz（ZIP）读写
src/wav.rs         WAV/PCM
src/win.rs         WASAPI 回环 / 覆盖层
src/serve.rs       管理 UI + API
src/static/        内嵌网页
```

## 许可

[GPL-3.0-only](LICENSE)

## 声明

- 仅回环采集本机扬声器，不上传网络；管理端只绑定 127.0.0.1。
- 音效库由使用者自行准备；请勿分发含有他人版权音频的 `library.srz`。
