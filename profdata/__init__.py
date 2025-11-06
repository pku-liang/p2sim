import numpy as np
import struct
from dataclasses import dataclass

class Loader:
    def __init__(self, data: bytes):
        self.data = data
        self.ptr = 0
    def load_uint32(self):
        value, = struct.unpack_from('i', self.data, self.ptr)
        self.ptr += 4
        return value
    def load_uint64(self):
        value, = struct.unpack_from('q', self.data, self.ptr)
        self.ptr += 8
        return value
    def load_array(self, dtype: np.dtype, n = None):
        if n is None:
            n = self.load_uint64()
        value = np.frombuffer(self.data, dtype, n, self.ptr).reshape(n)
        self.ptr += n * np.dtype(dtype).itemsize
        return value

@dataclass
class ThreadProfData:
    # [blocks]
    act_cnt: np.ndarray
    # [blocks, width]
    act_hash: np.ndarray
    # [N * 6]
    rdtsc_ticks: np.ndarray

@dataclass
class ProfData:
    # [part] => ThreadProfData
    threads: list[ThreadProfData]
    # [thread, block, orig_block]
    obr: np.ndarray

def load_profdata(filename):
    with open(filename, 'rb') as f:
        data = f.read()
    loader = Loader(data)
    act_hash_width = loader.load_uint32()
    part_count = loader.load_uint64()
    th = []
    for _ in range(part_count):
        act_cnt = loader.load_array(np.uint32)
        act_hash_cnt = loader.load_uint64()
        act_hash = loader.load_array(np.uint8, act_hash_cnt * act_hash_width).reshape(act_hash_cnt, act_hash_width)
        rdtsc_ticks = loader.load_array(np.uint64)
        th.append(ThreadProfData(act_cnt, act_hash, rdtsc_ticks))
    obr_len = loader.load_uint64()
    obr = loader.load_array(np.uint32, obr_len * 3).reshape(obr_len, 3)
    return ProfData(th, obr)
