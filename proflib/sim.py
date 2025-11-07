import numba

@numba.jit(nopython=True, boundscheck=False)
def minhash_sim_matrix_impl(act_hash, res):
    N, _ = act_hash.shape
    for i in range(N):
        for j in range(N):
            res[i, j] = (act_hash[i] == act_hash[j]).sum()

def minhash_sim_matrix(act_hash):
    N, _ = act_hash.shape
    res = np.empty((N, N), dtype=np.uint8)
    minhash_sim_matrix_impl(act_hash, res)
    return res

