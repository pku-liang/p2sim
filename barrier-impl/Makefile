all: $(patsubst %.cc,%.exe,$(wildcard *.cc))

%.exe: %.cc
	clang++ -std=c++20 -pthread -O2 -o $@ $<