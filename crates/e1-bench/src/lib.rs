pub mod backend;
pub mod runner;
pub mod synthetic;
pub mod templates;
pub mod types;

pub use runner::run_benchmark;
pub use types::{BenchmarkOptions, CircuitKind, Profile};

pub mod circuits {
    pub mod batch_sig {
        pub use crate::templates::{invalid_c4_tampered, prove_inner};
    }

    pub mod certificate {
        pub use crate::templates::{build_template, prove_inner};
    }

    pub mod geofence {
        pub use crate::templates::invalid_c1_outside;
    }

    pub mod nullifier {
        pub use crate::templates::invalid_c5_duplicate;
    }

    pub mod threshold {
        pub use crate::templates::invalid_c3_overflow;
    }
}

pub mod recursive {
    pub mod wrapper {
        pub use crate::templates::{wrapper_witness, TemplateCache};
    }
}

pub mod bench {
    pub mod runner {
        pub use crate::runner::run_benchmark;
    }
}

pub mod utils {
    pub mod data_gen {
        pub use crate::synthetic::synthetic_lot;
    }
}
