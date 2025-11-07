import numpy as np
import matplotlib.pyplot as plt

method_names = [
    'bincount-method',
    'binsample-method',
    'minhash-method',
    'ground-truth',
]

ground_truth = np.load('ground-truth.npy')

fig, axes = plt.subplots(2, 2, figsize=(10, 10))
axes = axes.flatten()

for i, name in enumerate(method_names):
    data = np.load(f'{name}.npy')
    if name == 'minhash-method':
        data = data ** 0.7
    diff = np.abs(data - ground_truth)
    rmse = np.sqrt((diff ** 2).mean())
    abs_err = diff.mean()
    axes[i].matshow(data, cmap='viridis')
    axes[i].set_title(f'{name} (RMSE: {rmse:.4f})')
plt.savefig('comparison.png', dpi=200)
