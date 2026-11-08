# Linux 启动到用户态验证（hina，2026-10-28）

验证通过：现有 `exe/linux.bin`（Linux 4.18.0-g655055af-dirty）在 baseline 和修复版 Rocket T24 上均启动并执行 `/init`，输出：

```text
Hello, RISC-V World!
hanging
```

去掉仿真频率行后，两个程序从装载镜像到用户态输出的完整文本逐字相同。没有 RTL 断言、Kernel panic、Oops 或 BUG 报错。日志与运行状态见 [report.json](report.json)。

## 频率

同一轮、同一个 23,500,000 周期采样点：

| 程序 | 累计平均仿真频率 |
| --- | ---: |
| baseline | 126.644 kHz |
| 修复版 | 285.269 kHz |

修复版 / baseline = **2.253x**。这是共享主机上的一次配对测量，不是三轮中位数；先前 2000 万周期配对测得 115.146 → 222.592 kHz（1.933x）。

## 成功判据和停止方式

从镜像的 CPIO initramfs 提取到 1544 字节的 RISC-V ELF `/init`。反汇编显示它执行两次 Linux `write` 系统调用，随后在自身地址无限跳转；详见 [init-disassembly.txt](init-disassembly.txt)。因此 `hanging` 是程序预期输出，不是仿真器卡死或内核故障。

测试上限为 1 亿周期。监控脚本观察到两行完整用户态输出后，向本次启动的进程组发送 SIGTERM，避免继续运行无意义的死循环。因此 report 中 returncode=-15 表示主动结束，不是仿真断言失败。baseline 最后采样 2350 万周期，修复版因监控轮询延迟又运行到 2360 万周期；频率比较统一使用 2350 万周期。

镜像 SHA-256：`b78d2853698703765b4a958a9cc4364524242626a13cfe9faaae15db73e11665`。

模拟器 SHA-256、原始日志哈希和运行时间均记录于 report.json。执行脚本为 [boot-to-init.py](boot-to-init.py)，原始日志为 [baseline.out](baseline.out) 和 [fixed.out](fixed.out)。远端对应目录为 `~/pro/p2sim-next/p2sim/build/linux-boot-to-init-20261028`。

这验证了该最小 Linux 镜像的内核启动、用户态入口和实际系统调用输出；没有把它扩展为完整发行版、交互式 shell 或任意 Linux 工作负载的验证结论。
