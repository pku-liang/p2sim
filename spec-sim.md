# Half-Sync Speculative Simulation

## 核心思想

传统并行周期仿真通常在每个周期末执行一次全局 barrier：各线程完成 `t` 周期计算并发送事件，然后等待所有线程到达，最后统一 commit，开始 `t+1` 周期。

Half-sync 方法将 barrier 拆成 `arrive` 和 `wait` 两个阶段，并利用两者之间原本用于等待慢线程的时间，提前执行 `t+1` 周期的大部分逻辑：

- 大部分电路由纯组合逻辑和普通标量状态构成，可以基于 partial state 提前运行；
- 少部分逻辑必须看到完整状态才能运行，例如 memory/array 及其相关逻辑；
- 当前周期尚未全部到达的事件在 barrier 完成后补充 commit；
- speculative 结果写入下一周期的 event queue，不会提前修改完整的 architectural state。

其目标不是取消周期同步语义，而是用下一周期的有用计算覆盖 `barrier->arrive` 到 `barrier->wait` 之间的同步等待，实现通信与计算 overlap。

## 稳态执行流程

设：

- `S(t)`：周期 `t` 开始时的完整状态；
- `Q(t)`：执行周期 `t` 时产生、用于构造 `S(t+1)` 的事件队列；
- `Q(t+1)`：speculative 执行下一周期时产生的事件队列；
- `speculative blocks`：可以基于 partial state 提前执行的多数 block；
- `non-speculative blocks`：必须等待完整状态的少数 block。

稳态循环如下：

```cpp
void task() {
  for (uint64_t t = 0; t < cycles; t++) {
    // speculative(t) 已经在上一轮 barrier 等待阶段执行。
    // 此处只补跑依赖完整 S(t) 的少量逻辑。
    eval_non_speculative(t, state[t], queue[t & 1]);
    flush_and_publish(t);

    barrier.arrive(t);

    // queue(t) 仍可能收到慢线程发布的事件，只消费当前已发布部分。
    commit_partial(t, queue[t & 1]);

    // 得到 partial S(t+1) 后，提前执行大部分下一周期逻辑。
    eval_speculative(t + 1, partial_state[t + 1], queue[(t + 1) & 1]);

    // speculative 结果已经保存在下一代 queue 中，不 commit architectural state。
    barrier.wait(t);

    // barrier 完成后 queue(t) 不再有生产者，补齐剩余事件。
    commit_rest(t, queue[t & 1]);
    // 此时得到完整的 S(t+1)。
  }
}
```

第一周期需要额外执行尚未预计算的 speculative 部分；最后一个周期需要 drain 尚未 commit 的队列。这是流水线的 prologue 和 epilogue，稳态不承担这部分开销。

## 双 event queue

必须同时保留两个不同 generation 的 event queue：

| Queue | 作用 | 在当前 barrier 完成前是否可复用 |
|---|---|---|
| `Q(t)` | 接收周期 `t` 产生的事件，用于构造 `S(t+1)` | 不可复用；慢线程仍可能向其追加事件 |
| `Q(t+1)` | 接收提前执行的周期 `t+1` 事件，用于构造 `S(t+2)` | 不可 commit，但可以继续接收 speculative 事件 |

每个 queue 自身又经历两次 commit：

1. `commit_partial`：在本线程 `arrive` 后，消费当前已经完整发布的事件；
2. `commit_rest`：在全局 `wait` 完成后，从 commit cursor 继续消费最终剩余事件。

因此该方法同时包含：

- **double buffering**：`Q(t)` 与 `Q(t+1)` 按周期轮换；
- **two-phase commit**：同一个 queue 先 partial commit，再 rest commit。

`Q(t)` 只有在所有生产者完成、所有事件被消费后才能清空并作为后续 generation 的 queue 复用。队列必须携带 generation 信息，避免晚到事件落入已经轮换的 queue。

## 哪些电路可以 speculative

设计假设是：**speculative block 占电路的大多数，non-speculative block 只占少数。**

通常可以 speculative 执行的逻辑包括：

- 纯组合逻辑；
- 普通标量寄存器的 next-state 计算；
- 输入在 partial commit 后已经 finalized 的 block；
- 不依赖 memory、array 或其他未完成状态的逻辑；
- 输出只写入下一代 event queue、没有不可逆副作用的逻辑。

通常需要等待完整状态的逻辑包括：

- memory/array 的读写；
- 涉及地址相关性、读写冲突和写入顺序的逻辑；
- 依赖尚未 commit 的 memory 或跨线程状态的 block；
- DPI、外部 I/O、不可撤销的打印或其他带副作用操作；
- 必须解析完多写者优先级后才能确定结果的逻辑。

Memory 是最典型的 non-speculative 部分。若希望 memory 也参与 speculation，通常需要额外的版本化、冲突检测、store buffer 或 rollback，复杂度会显著上升。Half-sync 的基本版本选择直接延后这部分逻辑。

## 正确性条件

### 1. Speculative 输入必须稳定

对任意 speculative block `B`，`commit_rest(t)` 不得再修改 `B` 在计算 `t+1` 时读取的输入。

等价地说：

```text
read_set(B) ∩ unfinished_state(t+1) = ∅
```

如果某个 late event 可能改变 block 的输入，则该 block 以及必要的依赖链必须归入 non-speculative 集合；或者实现显式 invalidation 和重新执行。

基本 half-sync 方案应优先采用保守分类，从而避免 rollback。

### 2. Speculative 输出不能提前可见

`eval_speculative(t+1)` 的结果只能进入 `Q(t+1)`，不能在 `barrier.wait(t)` 前修改完整的 `S(t+1)`、触发外部 I/O 或被当作已提交结果观察。

### 3. Partial commit 必须只消费已发布事件

队列可能存在已经 reserve 但尚未写完的 entry。`commit_partial` 必须区分：

- 已经完整发布、可以读取的 entry；
- 仍由生产者填写、不可读取的 entry；
- barrier 完成后确定的 final tail。

典型实现需要 publish bit、ready flag 或 reserve/publish 分离的 tail。

### 4. Barrier 必须提供内存序

- 生产者发布事件后以 release 语义执行 `arrive`；
- 消费者读取已发布事件时使用 acquire 语义；
- `wait` 返回后，必须能观察到所有线程在当前 generation 中发布的事件。

### 5. Commit 顺序必须保持 RTL 语义

两次 commit 不能改变多写者优先级、nonblocking assignment 语义或同周期事件的确定性。即使事件到达时间不同，最终 `S(t+1)` 也必须与无 speculation 的参考执行一致。

## 通信与计算 overlap

设某线程执行 `arrive` 后，还需要等待慢线程时间 `T_wait`；partial commit 和可提前执行的计算耗时为：

```text
T_overlap = T_partial_commit + T_speculative_eval
```

则暴露出来的纯 barrier stall 约为：

```text
T_exposed_wait = max(0, T_wait - T_overlap)
```

可隐藏的同步代价上限为：

```text
min(T_wait, T_overlap)
```

实际收益还需扣除：

- 第二次 commit 的固定开销；
- 双 queue 带来的额外 cache footprint；
- state completeness、generation 和 publish metadata；
- speculative/non-speculative 分类导致的调度开销。

该方法最适合以下情况：

- barrier 到达时间存在明显 skew；
- speculative block 占比高；
- memory 等 non-speculative 路径较短；
- event queue 的 partial drain 成本较低；
- 下一周期计算量足以覆盖同步延迟。

如果各线程几乎同时到达 barrier，或大部分逻辑依赖 memory，overlap 空间很小，两次 commit 反而可能增加开销。

## 实现要领

1. 以 destination thread 为 event queue owner：远端线程是 producer，本线程是唯一 consumer，可简化 partial commit。
2. 为每个 worker 准备两个 queue generation，并显式维护 `write_tail`、`published_tail`、`commit_cursor` 和 `final_tail`。
3. 静态计算 speculative block 集合；默认将 memory/array 及其未稳定依赖放入 non-speculative 集合。
4. 在调试版本中维护 state finalized/dirty mask，验证 speculative block 没有读取 unfinished state。
5. 使用无 speculation 的单线程或严格 barrier 模式作为 reference，逐周期比较 state 和 event queue。
6. 分别统计 `arrive->wait` 时间、partial/rest commit 时间、speculative 覆盖率和真正暴露的 wait 时间，避免只观察总运行时间。

## 方法本质

Half-sync 可以看作在周期边界上建立一条两级流水线：

```text
当前周期：完成少量 must-wait 逻辑 -> 发布 Q(t)
同步窗口：partial commit Q(t) -> 提前执行大部分 t+1 逻辑 -> 写 Q(t+1)
同步完成：补齐 Q(t) -> 得到完整 S(t+1)
```

它仍然保持严格的周期级提交顺序，但允许纯计算跨越 barrier 的等待区间。性能来源不是减少计算量，而是把原本串行的“通信等待”和“下一周期大部分计算”重叠起来。

## 当前实现

本仓库的 `--spec-sim` 代码生成模式实现了：

- split `arrive`/`wait` barrier；
- 两份跨线程 shared-state buffer，各自保存待发布掩码；状态变化时标记两份缓冲，只有目标缓冲尚未收到最新值时才复制，复制本身不产生额外激活事件；
- 每个 partition 的 current/next 双 activation queue；
- memory/array、remote-input 前缀、always block 和 observer 的保守分类；
- barrier 前 speculative pass；
- barrier 后由 late pull 重新激活受影响 block 的 repair pass；
- observer 与其余 repair block 按原始 block 顺序执行，保留读旧寄存器值后再写新值的依赖。

安全模式默认把 remote mirror reader 作为 non-speculative 根。只有显式传入 `--spec-allow-remote` 才允许更激进的 remote-dependent speculation；该模式必须重新做 correctness 验证。

`gAssert` 在所有模式下保持启用，失败时无条件终止，包括使用 `NDEBUG` 的构建。observer 不参与 early pass，但必须留在 repair pass 的原始位置；将它们统一移到周期末会越过 read-before-write 依赖，读到下一周期的寄存器值。`classify_speculative_part` 沿同周期依赖（包括读旧值到写新值的反依赖）传播禁止早执行约束，确保 observer 的后继不会提前修改其输入。

`tests/observer-order.py` 检查 read-before-write 顺序、跨调用的奇偶周期批次，以及故意失败的 RTL 断言；覆盖 baseline、spec 和 no-speculate control 的 `step`/`step_mt`。完整 Rocket 验证还应检查 Dhrystone 的全部最终值、CoreMark CRC 和 500 万周期完成情况。

推荐生成配置：

```bash
target/release/ggsim \
  designs/rocket_m_1.ggsim.sexp \
  emu/emu.cc \
  -lfesvr \
  -o build/rocket_m_1.T24.spec \
  -k 24 \
  --prof-rdtsc-avg \
  --spec-sim
```

由于 hina 是共享开发机，后台负载会让绝对仿真频率明显波动。性能门槛必须使用同一时段、同一线程数和同一周期数紧邻运行的 baseline/spec 日志比较，不能将不同时段的峰值直接相除。

历史版本（关闭 spec RTL 断言）的 5,000,000 周期配对审计结果；不可用作本次修复的正确性证据：

| Benchmark | T24 baseline | T24 spec-sim | 提升 |
|---|---:|---:|---:|
| Dhrystone | 129.254 kHz | 169.478 kHz | 31.12% |
| CoreMark | 125.234 kHz | 179.389 kHz | 43.24% |

对应门槛检查命令为：

```bash
scripts/compare-benchmarks.py \
  build/rocket_m_1.T24.base/bench-audit/dhry.out \
  build/rocket_m_1.T24.spec-final/bench-audit/dhry.out

scripts/compare-benchmarks.py \
  build/rocket_m_1.T24.base/bench-audit/coremark.out \
  build/rocket_m_1.T24.spec-final/bench-audit/coremark.out
```

两项检查均以默认 `--minimum-percent 30` 返回 0。

Dhrystone 完整输出最终变量且全部符合期望。CoreMark 的 CRC 为：

```text
crclist   = 0xe714
crcmatrix = 0x1fd7
crcstate  = 0x8e3a
```

三项均匹配 CoreMark known-id 3。当前镜像固定运行 10 iterations，模拟时间少于官方要求的 10 秒，因此仍打印时长警告；该警告不表示 CRC 或计算错误。


## 2026-10-28 正确性修复后的验证

恢复 RTL 断言并保留 observer 的读写顺序后，进一步采用每个缓冲区独立的待发布掩码，避免稳定共享状态的重复复制。hina Rocket T24 的 Dhrystone/CoreMark 均完成三次 500 万周期运行，全部最终变量/CRC 和 RTL 断言检查通过。

| Benchmark | baseline 中位数 | 修复版中位数 | 加速比 | 相对同轮原 spec |
|---|---:|---:|---:|---:|
| Dhrystone | 121.888 kHz | 188.727 kHz | 1.548x | +20.1% |
| CoreMark | 129.576 kHz | 201.427 kHz | 1.555x | +13.3% |

原始日志、失败的第一版审计、运行方法和验证范围见 [正确性与性能审计](results/correctness-20261028/README.md)。双缓冲回归入口为 `tests/publication-buffers.py`，完整 benchmark 审计入口为 `scripts/audit-spec-benchmarks.py`。


## Linux 用户态启动验证

2026-10-28 在 hina 使用现有 `exe/linux.bin`（Linux 4.18）完成 Rocket T24 的 baseline/修复版启动验证。两版均执行 `/init` 并打印 `Hello, RISC-V World!`、`hanging`，归一化完整输出一致且没有 RTL 断言失败。`hanging` 来自镜像中 `/init` 的主动死循环。

同一轮的 2350 万周期采样点为 baseline 126.644 kHz、修复版 285.269 kHz（2.253x，单次测量）。运行范围、停止方式及证据见 [Linux 启动验证](results/linux-boot-to-init-20261028/README.md)。
