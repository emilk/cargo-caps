use rand::Rng;

/// Performs reservoir sampling on an iterator, selecting N random items.
///
/// This implementation uses Algorithm R from Knuth's "The Art of Computer Programming".
/// It processes the iterator in a single pass with O(N) space complexity.
pub fn reservoir_sample<I, T, R>(iter: I, n: usize, rng: &mut R) -> Vec<T>
where
    I: IntoIterator<Item = T>,
    R: Rng + ?Sized,
{
    let mut reservoir = Vec::with_capacity(n);
    let mut iter = iter.into_iter().enumerate();

    // Fill the reservoir with the first n items
    while reservoir.len() < n
        && let Some((_, item)) = iter.next()
    {
        reservoir.push(item);
    }

    // For each subsequent item, randomly decide whether to include it
    for (i, item) in iter {
        let j = rng.random_range(0..=i);
        if j < n {
            reservoir[j] = item;
        }
    }

    reservoir
}

/// Iterator extension trait for convenient reservoir sampling
pub trait ReservoirSampleExt: Iterator {
    /// Take a reservoir sample of N items from this iterator
    fn reservoir_sample_with<R: Rng + ?Sized>(self, n: usize, rng: &mut R) -> Vec<Self::Item>
    where
        Self: Sized,
    {
        reservoir_sample(self, n, rng)
    }

    /// Take a reservoir sample of N items from this iterator
    fn reservoir_sample(self, n: usize) -> Vec<Self::Item>
    where
        Self: Sized,
    {
        self.reservoir_sample_with(n, &mut rand::rng())
    }
}

impl<I: Iterator> ReservoirSampleExt for I {}
