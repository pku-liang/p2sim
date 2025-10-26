import numpy as np
import struct
import matplotlib.pyplot as plt
import seaborn as sns
import sys
import re
from pathlib import Path

target_dirs = sorted(list(map(Path, sys.argv[1:-1])), key=lambda x: int(x.suffix[2:]))
save_fig = sys.argv[-1]
print(target_dirs, save_fig)

n_cols = 8
n_rows = (len(target_dirs) + 1) // 2

fig, axes = plt.subplots(n_rows, n_cols, figsize=(4 * n_cols, n_rows * 5))
axes = axes.reshape(n_rows, 2, 4).transpose(1, 0, 2).reshape(-1, 4)
plt.subplots_adjust(left=0.05, right=0.95, bottom=0.2 / len(target_dirs), top=1 - 0.2 / len(target_dirs))

total_ticks = []
all_freqs = []

for col, target_dir in enumerate(target_dirs):

    with open(f'{target_dir}/prof.log', 'r') as f:
        freqs = re.findall(r'cycle=\d+, freq=([0-9.]+) kHz', f.read())
        freq = max(map(float, freqs))
        all_freqs.append(freq)

    try:

        with open(f'{target_dir}/prof.bin', 'rb') as f:
            data = f.read()

        threads, = struct.unpack('q', data[:8])
        data = np.frombuffer(data[8:], dtype=np.uint64).reshape(threads, -1, 6)
        total_ticks.append(data[0].max() - data[0].min())
        data = data[:, 150000:, :] - data[:, 150000, None, 0, None]
        print(target_dir, total_ticks)
        cost = np.diff(data, axis=-1)
        avg_cost = cost.mean(axis=1)
        names = [
            '1. barrier',
            '2. pull',
            '3. step',
            '4. barrier',
            '5. push',
        ]

        part1 = cost[:, :10]
        part2 = cost[:, 100000:100010]
        for i, part in enumerate([part1, part2]):
            plt.sca(axes[col, i])
            offseted = part.reshape(part.shape[0], -1)
            pos = np.arange(offseted.shape[0])
            palette = sns.color_palette('muted', 6)
            for p in range(offseted.shape[1]):
                plt.barh(
                    pos,
                    offseted[:, p],
                    left=offseted[:, :p].sum(axis=1),
                    color=palette[p % len(names)],
                    height=0.3,
                    label=names[p] if p < len(names) else None
                )
                if p % len(names) == 0:
                    plt.axvline(offseted[:, :p].sum(axis=1).mean(), color='black', lw=0.5)
        
        plt.sca(axes[col, 2])
        plt.pie(avg_cost.mean(axis=0), labels=names, autopct='%1.1f%%')

        plt.sca(axes[col, 3])
        x = np.arange(cost.shape[0])
        items = []
        total_cost = np.sum(avg_cost, axis=-1)
        for i in range(5):
            bottom = np.sum(avg_cost[:, :i], axis=-1)
            item = plt.bar(x, avg_cost[:, i], bottom=bottom, width=0.3)
            for j in range(cost.shape[0]):
                pct = avg_cost[j, i] / total_cost[j]
                plt.text(x[j], bottom[j] + avg_cost[j, i] / 2, f'{pct:.0%}', ha='center', va='center')
            items.append(item)
        plt.xticks(x, x)
    except Exception as e:
        print(e)

plt.savefig(f'{save_fig}-prof.png', dpi=100)

fig, axes = plt.subplots(1, 2, figsize=(10, 5))
names = [target_dir.suffix for target_dir in target_dirs]
plt.sca(axes[0])
if len(total_ticks):
    plt.barh(names, total_ticks)
plt.sca(axes[1])
plt.barh(names, all_freqs)
plt.savefig(f'{save_fig}-ticks.png', dpi=100)
