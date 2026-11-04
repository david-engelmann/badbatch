//! badbatch. Stamps what a run left behind and walks the stamp
//! graph.
//!
//! Badbatch stamps what a run left behind. It never records
//! the run itself. A stamp is built from a run record another
//! tool left, and the recall walk and the execution plan read
//! the stamp against a recorded current state and report only.

pub mod errors;
pub mod judgment;
pub mod paths;
pub mod plan;
pub mod pyjson;
pub mod pytext;
pub mod records;
pub mod stamps;
pub mod walk;

mod printable_table;

pub use errors::{Error, Result};
pub use judgment::{
    CANDIDATES, CONFIDENCE_THRESHOLD, FixturePlayer, JUDGMENT_POINT, JudgmentSource, RANKING_POINT,
    check_answer, check_ranking_answer, classification_request, ranking_request, trusted,
    trusted_score, validate_fixture,
};
pub use plan::{build_plan, candidates_for, format_plan};
pub use records::{ACTIONS, CLASSES, UNCLASSED, content_digest, parse_run_record};
pub use stamps::{build_stamp, format_stamp, parse_stamp};
pub use walk::{format_recall, parse_current_state, recall_walk};
