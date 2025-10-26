#!/bin/bash

freq=2800
for cpu in $(seq 48 68) $(seq 144 164); do
    echo "locking freq to $freq for cpu $cpu"
    echo $freq > /sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_max_freq
    echo $freq > /sys/devices/system/cpu/cpu$cpu/cpufreq/scaling_min_freq
done
