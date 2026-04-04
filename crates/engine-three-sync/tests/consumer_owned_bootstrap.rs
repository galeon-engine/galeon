// SPDX-License-Identifier: AGPL-3.0-only OR Commercial

use galeon_engine::{Engine, MaterialHandle, MeshHandle, Plugin, Transform, Visibility};
use galeon_engine_three_sync::{WasmEngine, WasmFramePacket};

const TEST_MESH_ID: u32 = 7;
const TEST_MATERIAL_ID: u32 = 11;

struct SceneBootstrapPlugin;

impl Plugin for SceneBootstrapPlugin {
    fn build(&self, engine: &mut Engine) {
        engine.world_mut().spawn((
            Transform {
                position: [1.25, 0.5, -0.75],
                rotation: [0.0, 0.0, 0.0, 1.0],
                scale: [2.0, 0.25, 2.0],
            },
            Visibility { visible: true },
            MeshHandle { id: TEST_MESH_ID },
            MaterialHandle {
                id: TEST_MATERIAL_ID,
            },
        ));
    }
}

struct DemoWasmEngine {
    inner: WasmEngine,
}

impl DemoWasmEngine {
    fn new() -> Self {
        let mut inner = WasmEngine::new();
        inner.engine_mut().add_plugin(SceneBootstrapPlugin);
        Self { inner }
    }
}

fn assert_bootstrap_frame(frame: WasmFramePacket) {
    assert_eq!(frame.entity_count(), 1);
    assert_eq!(frame.mesh_handles(), vec![TEST_MESH_ID]);
    assert_eq!(frame.material_handles(), vec![TEST_MATERIAL_ID]);
    assert_eq!(frame.visibility(), vec![1]);
    assert_eq!(frame.transforms().len(), 10);
}

#[test]
fn consumer_owned_wrapper_can_seed_first_frame_via_plugin() {
    let frame = DemoWasmEngine::new().inner.extract_frame();
    assert_bootstrap_frame(frame);
}

#[test]
fn consumer_owned_wrapper_can_wrap_a_prebuilt_engine() {
    let mut engine = Engine::new();
    engine.add_plugin(SceneBootstrapPlugin);

    let frame = WasmEngine::from_engine(engine).extract_frame();
    assert_bootstrap_frame(frame);
}
