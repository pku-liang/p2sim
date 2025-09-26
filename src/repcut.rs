use std::cmp::Ordering;

use ordered_float::OrderedFloat;
use rayon::iter::{IntoParallelIterator, ParallelIterator};

use crate::ir::*;

pub struct HyperGraph {
    pub nodes: Vec<LogicId>,
    pub edges: Vec<Vec<LogicId>>,
    pub edge_weights: Vec<usize>,
    pub node_weights: Vec<usize>,
    pub node_view: Vec<Vec<(usize, usize)>>,
}

impl HyperGraph {
    pub fn similarity(&self, a: usize, b: usize) -> f64 {
        iou(&self.node_view[a], &self.node_view[b])
    }
}

pub fn iou(a: &[(usize, usize)], b: &[(usize, usize)]) -> f64 {
    let mut union = 1;
    let mut inters = 1;
    let mut i = 0;
    let mut j = 0;
    while i < a.len() && j < b.len() {
        match a[i].0.cmp(&b[j].0) {
            Ordering::Equal => {
                inters += a[i].1.min(b[j].1);
                union += a[i].1.max(b[j].1);
                i += 1;
                j += 1;
            }
            Ordering::Less => {
                union += a[i].1;
                i += 1;
            }
            Ordering::Greater => {
                union += b[j].1;
                j += 1;
            }
        }
    }
    while i < a.len() {
        union += a[i].1;
        i += 1;
    }
    while j < b.len() {
        union += b[j].1;
        j += 1;
    }
    inters as f64 / union as f64
}

pub fn compute_dup_cost(states: &[State], logic: &Logic) -> usize {
    let mut cost = 0;
    for state in &logic.owned {
        let state = &states[state.index()];
        let prod_dims = state.dims.iter().fold(1, |acc, x| acc * *x);
        cost += (state.width + 7) / 8 * prod_dims as usize;
    }
    cost
}

pub fn build_hyper_graph(design: &Design) -> HyperGraph {
    let logics = &design.logics;
    let mut nodes = vec![];
    let mut node_weights = vec![];
    let mut edges = vec![vec![]; logics.len()];
    let mut edge_weights = vec![0; logics.len()];
    for i in (0..logics.len()).rev() {
        let weight = compute_dup_cost(&design.states, &logics[i]);
        if logics[i].next.is_empty() {
            nodes.push(logics[i].id);
            node_weights.push(weight);
        }
        let mut cur_edges: Vec<LogicId> = vec![];
        for next in &logics[i].next {
            cur_edges.extend(edges[next.index()].iter().cloned());
        }
        cur_edges.sort();
        cur_edges.dedup();
        edges[i] = cur_edges;
        edge_weights[i] = weight;
    }
    nodes.reverse();
    node_weights.reverse();
    let mut node_view = vec![vec![]; nodes.len()];
    for (i, (e, w)) in edges.iter().zip(edge_weights.iter()).enumerate() {
        for edge in e {
            node_view[edge.0 as usize].push((i, *w));
        }
    }
    HyperGraph {
        nodes,
        edges,
        edge_weights,
        node_weights,
        node_view,
    }
}

pub fn dist_matrix(graph: &HyperGraph) -> Vec<Vec<f64>> {
    (0..graph.nodes.len())
        .into_par_iter()
        .map(|i| {
            (0..graph.nodes.len())
                .map(|j| graph.similarity(i, j))
                .collect()
        })
        .collect()
}

pub fn mst(dist: &[Vec<f64>]) -> Vec<(usize, usize, f64)> {
    let mut min_dist = vec![f64::MAX; dist.len()];
    let mut prev = vec![None; dist.len()];
    let mut used = vec![false; dist.len()];
    for _ in 0..dist.len() {
        let u = min_dist
            .iter()
            .enumerate()
            .filter(|(x, _)| !used[*x])
            .min_by_key(|(_, d)| OrderedFloat(**d));
        let Some((u, _)) = u else { break };
        used[u] = true;
        for v in 0..dist.len() {
            if !used[v] && dist[u][v] < min_dist[v] {
                min_dist[v] = dist[u][v];
                prev[v] = Some(u);
            }
        }
    }
    let mut res = vec![];
    for (i, p) in prev.iter().enumerate() {
        if let Some(p) = p {
            res.push((*p, i, min_dist[i]));
        }
    }
    res
}

pub struct UFP {
    parent: Vec<usize>,
    rank: Vec<u8>,
}

impl UFP {
    pub fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }
    pub fn find(&self, mut x: usize) -> usize {
        while self.parent[x] != x {
            x = self.parent[x];
        }
        x
    }
    pub fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        if self.rank[rx] < self.rank[ry] {
            self.parent[rx] = ry;
        } else if self.rank[rx] > self.rank[ry] {
            self.parent[ry] = rx;
        } else {
            self.parent[ry] = rx;
            self.rank[rx] += 1;
        }
    }
}

pub fn mst_partition(graph: &HyperGraph, threshold: f64) -> Vec<Vec<LogicId>> {
    let dist = dist_matrix(graph);
    let mst = mst(&dist);
    let mut ufp = UFP::new(graph.nodes.len());
    for (u, v, d) in mst {
        if d >= threshold {
            ufp.union(u, v);
        }
    }
    let mut res = vec![vec![]; graph.nodes.len()];
    for (i, p) in graph.nodes.iter().enumerate() {
        res[ufp.find(i)].push(*p);
    }
    res.into_iter().filter(|x| !x.is_empty()).collect()
}
