#!/bin/bash

target_dir=$1
shift

cat > $target_dir/Makefile <<EOF
CCACHE = ccache
CXX = clang++
CXXFLAGS += -O2 -pthread
LDFLAGS += -pthread -fuse-ld=lld

all: STestHarness

%.o: %.cc
	\$(CCACHE) \$(CXX) \$(CXXFLAGS) -c \$< -o \$@
EOF

objs=()
for cc in $target_dir/*.cpp; do
	name=$(basename $cc)
	name=${name%.cpp}.o
	objs+=($name)
done

for arg in $@; do
basename="$(basename $arg)-$(echo $arg | sha1sum | head -c 8)"
path=$(realpath $arg)
cat >> $target_dir/Makefile <<EOF
$basename.o: $path
	\$(CCACHE) \$(CXX) \$(CXXFLAGS) -I. -c \$< -o \$@ -I\$(CONDA_PREFIX)/include
EOF
objs+=($basename.o)
done

cat >> $target_dir/Makefile <<EOF
STestHarness: ${objs[@]}
	\$(CXX) -o \$@ $^ \$(LDFLAGS) \$(LDLIBS) -L\$(CONDA_PREFIX)/lib -lfesvr
EOF