#!/bin/bash

CURDIR=$PWD

just run-all

cd $CURDIR/baseline/gsim

just run-all

cd $CURDIR/baseline/vlt

just run-all