#[cfg(test)]
pub mod bootstrap;

#[cfg(test)]
pub(crate) mod fixtures;

#[cfg(test)]
mod backend_api;

#[cfg(test)]
mod capabilities;

#[cfg(test)]
mod auth_users;

#[cfg(test)]
mod share_secrets;

#[cfg(test)]
mod share_ownership;

#[cfg(test)]
mod user_writes;

#[cfg(test)]
mod enroll_gate;

#[cfg(test)]
mod register;

#[cfg(test)]
mod authz;

#[cfg(test)]
mod scenario_schema;

#[cfg(test)]
mod seeds;

#[cfg(test)]
mod torn_thumbnail;

#[cfg(test)]
mod matrix_negatives;

#[cfg(test)]
mod openapi_contract;

#[cfg(test)]
mod openapi_parity;

#[cfg(test)]
mod r6_session_revocation;
