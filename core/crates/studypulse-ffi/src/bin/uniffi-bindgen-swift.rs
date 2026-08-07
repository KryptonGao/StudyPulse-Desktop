// This tiny binary is the repository-local UniFFI generation entry point.  It
// delegates all binding semantics to the uniffi CLI so generated Swift types
// continue to reflect the annotated StudyPulseCore facade without duplicating
// binding logic in the application.

fn main() {
    uniffi::uniffi_bindgen_swift();
}
