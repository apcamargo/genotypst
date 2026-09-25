pub(crate) mod algorithms;
pub(crate) mod arcs;
pub(crate) mod config;
pub(crate) mod error;
pub(crate) mod fitting;
pub(crate) mod geometry;
pub(crate) mod output;
#[cfg(test)]
pub(crate) mod testing;
pub(crate) mod validation;

use config::LayoutConfig;
use error::LayoutError;
use output::AlgorithmLayout;
use validation::PairTable;

pub(crate) fn layout(
    config: &LayoutConfig,
    pair_table: &PairTable,
) -> Result<AlgorithmLayout, LayoutError> {
    algorithms::layout(config, pair_table)
}

pub(crate) fn fit(config: &[u8]) -> Result<Vec<u8>, fitting::FitError> {
    fitting::fit(config)
}
