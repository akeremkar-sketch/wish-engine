//! Game code as a user writes it: one system through the SDK's macro.

use sdk_spike::{Frame, Vec3};

#[macros_spike::system]
fn move_things(frame: &mut Frame) {
    let dt = frame.dt();
    let count = frame.count();
    let velocities: Vec<Vec3> = frame.column_mut::<Vec3>(1).to_vec();
    let positions = frame.column_mut::<Vec3>(0);
    for (p, v) in positions.iter_mut().zip(&velocities) {
        *p = *p + *v * dt;
    }
    // EDIT-MARKER: the build measurement changes this constant.
    let _ = count + 1;
}

sdk_spike::export!(move_things);
