# hina 正确性修复与性能审计（2026-10-28）

## 实现与基线

本地起点为 `9129a2a`；hina 的 `~/pro/p2sim-next/p2sim` HEAD 为 `4029b74`，附带原有 half-sync 工作区修改。现场核对全部 21 个 Rust 源文件与模板的哈希，和本地 HEAD 一致。simbench HEAD 为 `8fd71ae5a22fb05c0df6bfdca9079af165c8726e`。本次沿用 Rocket `rocket_m_1.ggsim.sexp`、T24、`--prof-rdtsc-avg`，未使用 `--spec-allow-remote`。

远端实验根目录：`~/pro/p2sim-next/p2sim/build/correctness-20261028`。本地 tmux 会话：`p2sim-correctness`。原有源代码修改、二进制及历史日志已保留；`source` 和 `source-sparse` 分别保存两次实验的源码。

修复包括：

- `gAssert` 在 baseline/spec/control 及 `NDEBUG` 构建中均启用，失败调用 `std::abort()`。
- observer 留在 repair pass 的原始顺序中。旧版把 observer 移至周期末，可能越过读旧值到写新值的依赖；单独的 observer 执行阶段已移除。
- 每个 shared-state buffer 独立跟踪待发布数据。变化标记两份缓冲，每份接收最新值后才清除自己的标记。刷新旧缓冲不会额外激活读者；串行/多线程切换时也不会丢失另一份缓冲的更新。

## 正确性验证

`tests/observer-order.py`：baseline/spec/no-speculate control 的 `step` 和 `step_mt`，含 1/3/4 周期批次、读旧值后写新值、故意失败的 RTL 断言。负例要求 `SIGABRT`，不能把普通非零退出当成断言生效。测试使用 `-DNDEBUG`。

`tests/publication-buffers.py`：T2 的三种模式，各运行 192 个批次，覆盖共享数据长期不变、跳变至零、双输入同时变化、奇偶批次，以及串行/多线程切换。输入图包含跨分区读者，实际生成 push/pull；只有本地读写的单线程寄存器不会覆盖这个路径。

最终补充验证：当前 observer 回归在原生成器下于 `order-spec` 收到 SIGABRT，确认旧错误仍被测试捕获。对通过的双缓冲生成物故意删除 buffer 1 的待更新标记传播，回归在 batch 8 报告实际值与期望值不符，确认测试能检测漏更新；见 `final-checks.log` 和 `publication-mutation.log`。

第一版完整 Rocket 测试与首轮审计的 18 次运行均完成 5,000,000 周期，Dhrystone 全部 22 项最终值（包括指针关系）和 CoreMark 三项 CRC 正确。CoreMark 镜像固定 10 iterations，保留原有不足官方 10 秒的时长警告；审计不把它混同于 CRC 错误。

## 性能方法

`scripts/audit-spec-benchmarks.py` 连续运行 baseline、原 spec、候选版，每个 workload 三轮，轮换顺序。每次记录二进制哈希、日志哈希、退出码、CPU affinity、主机负载和完整运行频率；先检查功能结果，再计算三轮中位数之比。脚本同时检查相对 baseline 的加速比和相对原 spec 的性能保留程度。

原二进制：`build/rocket_m_1.T24.base/sim`、`build/rocket_m_1.T24.spec-final/sim`。历史报告的 1.31x / 1.43x 来自关闭 spec RTL 断言的旧版本，不是本次正确性证据。

第一版仅修复 observer 与断言，结果见 `paired/report.json`：

| workload | baseline 中位数 kHz | 原 spec kHz | 修复版 kHz | 修复版 / baseline | 修复版 / 原 spec |
| --- | ---: | ---: | ---: | ---: | ---: |
| Dhrystone | 118.833 | 164.330 | 147.689 | 1.243x | 0.899x |
| CoreMark | 112.990 | 139.809 | 170.647 | 1.510x | 1.221x |

该版未通过性能门槛。随后增加双缓冲稀疏发布。曾尝试限制到 CPU 0–23，但 baseline 出现严重调度变慢，该次运行被主动终止，不作为性能证据；后续恢复历史实验的 CPU 调度范围。

最终稀疏发布版本通过审计，见 [`paired-sparse-full/report.json`](paired-sparse-full/report.json)。每项均为三次 5,000,000 周期运行的中位数，三个程序都使用 CPU 0–111 的原有调度范围：

| workload | baseline 中位数 kHz | 原 spec kHz | 最终修复版 kHz | 修复版 / baseline | 相对原 spec |
| --- | ---: | ---: | ---: | ---: | ---: |
| Dhrystone | 121.888 | 157.183 | 188.727 | 1.548x | +20.1% |
| CoreMark | 129.576 | 177.762 | 201.427 | 1.555x | +13.3% |

最终候选的六次完整运行均通过 RTL 断言与 benchmark 输出检查。两项加速比也均高于历史记录的 1.3112x / 1.4324x。主机仍为共享环境，数值是本次交错运行的测量结果。


## 复现

在 hina 的 p2sim Pixi 环境中运行：

```bash
pixi run python tests/observer-order.py /absolute/path/to/ggsim
pixi run python tests/publication-buffers.py /absolute/path/to/ggsim
pixi run python scripts/audit-spec-benchmarks.py \
  --baseline build/rocket_m_1.T24.base/sim \
  --previous build/rocket_m_1.T24.spec-final/sim \
  --candidate build/correctness-20261028/rocket.T24.sparse/sim \
  --output build/new-audit
```

结果只覆盖上述模式和输入；本次未验证 BOOM 或激进的 remote speculation，也不把 benchmark 通过视为对任意 RTL 的形式化证明。


Linux 补充验证也已完成：现有最小 Linux 4.18 镜像成功执行用户态 `/init`，两版输出一致。详见 [Linux 启动到用户态验证](../linux-boot-to-init-20261028/README.md)。
