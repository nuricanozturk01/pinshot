//! Memory hygiene: a capture holds tens of megabytes of screen pixels; give them back when it ends.

/// Return freed memory to the operating system now instead of whenever the allocator gets around to it.
pub fn release_unused() {
    // SAFETY: `mi_collect` has no preconditions; it only frees memory the allocator already considers unused.
    #[expect(
        unsafe_code,
        reason = "allocator maintenance call, no memory is touched"
    )]
    unsafe {
        libmimalloc_sys::mi_collect(true);
    }
}
