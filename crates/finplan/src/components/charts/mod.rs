//! Chart components for data visualization.

mod distribution;

pub use distribution::{
    render_distribution, render_historical_histogram, render_normal_distribution,
    render_regime_switching_distribution, render_student_t_distribution, render_sweep_histogram,
};
