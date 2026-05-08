/// Minimal Perfect Hash Function computation (stub).
pub fn compute_mph_seed(hashes: &[u64]) -> u64 {
    if hashes.is_empty() {
        return 0;
    }
    let len = hashes.len() as u64;
    for seed in 0..1000 {
        let mut slots = vec![false; hashes.len()];
        let mut collision = false;
        for &h in hashes {
            let slot = (h.wrapping_add(seed)) % len;
            if slots[slot as usize] {
                collision = true;
                break;
            }
            slots[slot as usize] = true;
        }
        if !collision {
            return seed;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mph_seed_no_collision() {
        let hashes = vec![1, 2, 3, 4, 5];
        let seed = compute_mph_seed(&hashes);
        let len = hashes.len() as u64;
        let mut slots = vec![false; hashes.len()];
        for &h in &hashes {
            let slot = (h.wrapping_add(seed)) % len;
            assert!(!slots[slot as usize], "Collision at slot {}", slot);
            slots[slot as usize] = true;
        }
    }

    #[test]
    fn mph_seed_empty() {
        let seed = compute_mph_seed(&[]);
        assert_eq!(seed, 0);
    }
}
