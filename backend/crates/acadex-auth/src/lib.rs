//! Authentication and authorisation shared by the API and worker:
//! Auth0 access-token verification and the role → permission matrix.

pub mod rbac;
mod verifier;

#[cfg(feature = "test-util")]
pub mod testing;

pub use rbac::{Permission, Role};
pub use verifier::{AuthError, Claims, UserInfo, Verifier};
