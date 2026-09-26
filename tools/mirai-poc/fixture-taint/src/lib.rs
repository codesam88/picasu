//! Source/sink modelling with MIRAI 1.1.12 annotations.
//!
//! MIRAI 1.1.12 has no built-in notion of untrusted input. The only taint
//! mechanism is its tag domain: `add_tag!` is the source, and the sink side is
//! a precondition that the value is not tagged. Each `t*` function below
//! isolates one step of that flow, because the tag turns out to survive some
//! boundaries and not others.
//!
//! Every case is checked at `--diag=verify`. See ../README.md for the results.

#![feature(generic_const_exprs)]
#![allow(incomplete_features)]
// `add_tag!` and friends expand to `cfg!(mirai)`, which is only set on the crate
// MIRAI is compiling, so check-cfg warns about an unknown cfg.
#![allow(unexpected_cfgs)]

use mirai_annotations::*;

/// Taint tag for request-derived (untrusted) values.
struct UntrustedTag<const MASK: TagPropagationSet> {}

type Untrusted = UntrustedTag<TAG_PROPAGATION_ALL>;

/// A Rocket-shaped request: the handler receives a query struct whose field is
/// attacker controlled.
pub struct GetScrollBarQuery {
    pub snapshot_id: Option<u32>,
}

/// Panic sink, modelled the way MIRAI requires. The unwrap is guarded by a
/// precondition that the value is not tainted, so a tainted value reaching this
/// function makes the precondition unsatisfiable.
fn read_scrollbar(id: Option<u32>) -> u32 {
    precondition!(does_not_have_tag!(&id, Untrusted));
    id.unwrap()
}

/// Same sink, one call deeper.
fn helper_read_scrollbar(id: Option<u32>) -> u32 {
    read_scrollbar(id)
}

/// Same sink, behind a `move` closure.
fn closure_read_scrollbar(id: Option<u32>) -> u32 {
    let call = move |value: Option<u32>| read_scrollbar(value);
    call(id)
}

/// A sanitizer a reader would expect to clear taint. Deliberately empty.
fn sanitize(_id: &Option<u32>) {}

/// T1: tag a local copy of the request field, then reach the sink through a
/// helper. Silent: the tag on the local is not observed at the sink.
pub fn t1_tagged_into_helper(query: GetScrollBarQuery) -> u32 {
    let id = query.snapshot_id;
    add_tag!(&id, Untrusted);
    helper_read_scrollbar(id)
}

/// T2: same, through a `move` closure. Silent, like T1.
pub fn t2_tagged_into_closure(query: GetScrollBarQuery) -> u32 {
    let id = query.snapshot_id;
    add_tag!(&id, Untrusted);
    closure_read_scrollbar(id)
}

/// T3: control for T1 and T2. The request value is never tagged, so the sink
/// precondition must hold.
pub fn t3_untagged_control(query: GetScrollBarQuery) -> u32 {
    helper_read_scrollbar(query.snapshot_id)
}

/// T4: tag, pass through a no-op by reference, then call the sink directly.
/// Silent, like T1, so the intervening reference call is not what drops the
/// tag.
pub fn t4_sanitizer_is_a_no_op(query: GetScrollBarQuery) -> u32 {
    let id = query.snapshot_id;
    add_tag!(&id, Untrusted);
    sanitize(&id);
    read_scrollbar(id)
}

/// T5: tag the request field place expression itself, then call the sink. This
/// is the one shape where the sink precondition fires.
pub fn t5_tagged_field(query: GetScrollBarQuery) -> u32 {
    add_tag!(&query.snapshot_id, Untrusted);
    read_scrollbar(query.snapshot_id)
}

/// T6: the tag is attached to the tainted binding, so a value computed
/// afterwards is untainted and the sink precondition must hold.
pub fn t6_taint_does_not_survive_rebinding(query: GetScrollBarQuery) -> u32 {
    let tainted = query.snapshot_id;
    add_tag!(&tainted, Untrusted);
    let fresh: Option<u32> = Some(7);
    read_scrollbar(fresh)
}

/// T7: T5 without the struct field, tagging a local instead. Silent.
pub fn t7_tagged_local_direct_sink(query: GetScrollBarQuery) -> u32 {
    let id = query.snapshot_id;
    add_tag!(&id, Untrusted);
    read_scrollbar(id)
}

/// T8: T7 with a no-op call in between. Silent, so T4 and T7 agree.
pub fn t8_intervening_ref_call(query: GetScrollBarQuery) -> u32 {
    let id = query.snapshot_id;
    add_tag!(&id, Untrusted);
    sanitize(&id);
    read_scrollbar(id)
}

/// T9: source and sink in the same function, no helper. Silent, so the losing of
/// the tag in T1 and T2 is not caused by the call boundary alone.
pub fn t9_inline_sink(query: GetScrollBarQuery) -> u32 {
    let id = query.snapshot_id;
    add_tag!(&id, Untrusted);
    precondition!(does_not_have_tag!(&id, Untrusted));
    id.unwrap()
}

/// T10: positive check instead of a negative precondition. Reports that the tag
/// on a local is not provably present, which is why T7 and T9 are silent.
pub fn t10_positive_check_on_local(query: GetScrollBarQuery) -> u32 {
    let id = query.snapshot_id;
    add_tag!(&id, Untrusted);
    verify!(has_tag!(&id, Untrusted));
    let _ = id;
    0
}

/// T11: tag the handler parameter directly and pass it by value. Reported. This
/// is the closest analogue of a Rocket handler forwarding a request field.
pub fn t11_tag_param_pass_by_value(id: Option<u32>) -> u32 {
    add_tag!(&id, Untrusted);
    read_scrollbar(id)
}
