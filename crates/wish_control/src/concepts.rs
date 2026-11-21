//! What agents (and people) coming from other engines reach for, and how
//! it is done here. Measured in the M4 agent evaluations: a capable model
//! asked for `Collider`, `Name`, `Visibility` and `Body` components that do
//! not exist. A plain "no such component" sends it guessing; saying what the
//! engine does instead does not.

/// How the engine does what a familiar but unknown name stands for.
pub fn explain(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    Some(match lower.as_str() {
        "name" | "tag" | "entityname" | "id" | "identifier" => {
            "Entities are named by their id (the key in the scene file); there is no Name component. Find entities by id pattern with entity_query \"name\": \"crate_*\"."
        }
        "visibility" | "visible" | "hidden" | "renderer" | "meshrenderer" | "mesh"
        | "meshfilter" | "staticmesh" => {
            "What is drawn comes from Primitive (built-in Cube, Sphere, Plane) or Model (a glTF asset); there is no Mesh or Visibility component. To hide something remove its Primitive or Model; describe_view says what is visible."
        }
        "boxcollider"
        | "spherecollider"
        | "capsulecollider"
        | "meshcollider"
        | "body"
        | "physics"
        | "physicsbody"
        | "characterbody"
        | "kinematicbody"
        | "staticbody"
        | "charactercontroller3d"
        | "rigidbody3d" => {
            "Physics: put a Collider on the entity (shape Box, Sphere, Capsule or Cylinder, sized by size and the entity's scale); add a RigidBody (kind Dynamic, Kinematic or Fixed) to make it move. Characters use a Kinematic RigidBody with CharacterController, CharacterMotion (wanted velocity) and CharacterState (grounded)."
        }
        "velocity" | "linearvelocity" | "angularvelocity" | "speed" => {
            "A rigid body's speed is BodyVelocity (linear m/s, angular rad/s): physics writes it every tick, and writing it sets the speed. A game may also declare its own Velocity component for things it moves itself."
        }
        "force" | "addforce" | "impulse" | "addimpulse" | "applyimpulse" => {
            "Push a dynamic body with BodyForce (a component: applied every tick while present) or kick it once with a BodyImpulse event (entity, linear, angular)."
        }
        "audiosource" | "audioclip" | "sound" | "sfx" | "music" | "audiosource3d"
        | "soundeffect" | "audioplayer" | "speaker" => {
            "Sound: put an AudioSource on an entity (clip \"assets/sounds/x.wav\" or .ogg, playing, looping, spatial) and an AudioListener on the camera; one-off sounds are PlaySound events. Sessions do not play sound: logs_get with kinds [\"audio\"] says what started and ended, how loud and where."
        }
        "audiolistener" | "listener" | "ears" => {
            "The listener is the AudioListener component (put it on the camera); the first one in entity order is heard from. Loudness and left/right are worked out from its position and orientation."
        }
        "animator" | "animation" | "animationclip" | "skeleton" | "skinnedmesh"
        | "animationplayer3d" => {
            "Animation: give an entity with a glTF Model an AnimationPlayer (clip: the clip's name in the file, time, speed, looping, playing). The engine moves time on every tick and sends AnimationFinished when a clip that does not loop ends."
        }
        "canvas" | "ui" | "gui" | "hud" | "widget" | "panel" | "label" | "textmesh"
        | "textmeshpro" | "uitext" | "uibutton" | "button" | "recttransform" | "control"
        | "menu" => {
            "Interface: an entity with a UiNode is a box (width, height, padding, direction Column or Row, position Absolute with offset, background); child entities with a UiNode are laid out inside it like flexbox. Add UiText (text, size, color), UiImage (a PNG) or UiButton to a box; clicks come as ButtonClicked events. Positions are pixels of a 1280x720 virtual screen (UiSettings); game code writes text with text(\"...\")."
        }
        "sprite" | "spriterenderer" | "sprite2d" | "camera2d" | "orthographic" | "ortho"
        | "tilemap" | "2d" | "ninepatch" | "nineslice" | "9slice" | "tint" => {
            "2D: use a Camera with projection Orthographic (ortho_height in meters) and entities with a Sprite (image: a PNG, rect: the frame in pixels, pixels_per_unit, layer, flip_x, smooth false for pixel art, color: the tint, size and slice: a nine-slice panel drawn at any size with fixed corners) and a Transform. Walk cycles come from a sprite sheet and a SpriteAnimator (see wish://docs/recipes, \"A walking character from a sprite sheet\"). For 2D physics set the PhysicsSettings resource's profile to SideView2D (a platformer) or TopDown2D (seen from above, no gravity)."
        }
        "animatedsprite" | "animatedsprite2d" | "spritesheet" | "spriteframes" | "flipbook"
        | "spriteanimation" | "animator2d" | "walkcycle" => {
            "Sprite animation: make a sprite sheet with asset_create (a .sheet.json path, sheet {image: a PNG, grid: {frame: [w, h], columns, rows} or frames: [[x, y, w, h]], clips: {walk: {frames: [0, 1, 2, 3], fps: 8}}}), then give the entity with a Sprite a SpriteAnimator {sheet, clip}. The engine moves it every tick and writes the frame shown into SpriteAnimator.frame; game code switches clips with animator.play(text(\"walk\")). wish://docs/recipes has \"A walking character from a sprite sheet\"."
        }
        "rigidbody2d" | "collider2d" | "boxcollider2d" | "circlecollider2d"
        | "capsulecollider2d" | "characterbody2d" | "physics2d" | "topdown" | "top-down" => {
            "2D physics: set the PhysicsSettings resource's profile to TopDown2D (seen from above: no gravity, characters slide along walls) or SideView2D (gravity along -Y). Then RigidBody and Collider work in the X-Y plane: a Sphere collider is a circle, a Box a rectangle (size.z unused), and a CharacterController moves a kinematic character with CharacterMotion.velocity."
        }
        "particles" | "particlesystem" | "particleemitter" | "emitter" | "cpuparticles"
        | "gpuparticles" | "particles2d" | "particles3d" | "vfx" => {
            "Particles: put a ParticleEmitter on an entity (rate, burst, lifetime, speed, spread_deg, gravity, size_start/size_end, color_start/color_end, seed). Particles keep no state; restart by writing start_time = Time.elapsed. ParticleState.alive counts them."
        }
        "followcamera" | "cameracontroller" | "thirdpersoncamera" | "camerafollow"
        | "springarm" | "cinemachine" | "virtualcamera" => {
            "No camera controller is built in: game code moves the camera (a FollowCamera component and a follow system). wish://docs/recipes has a working one to copy (\"A camera that follows\")."
        }
        "movingplatform" | "platform" | "elevator" | "lift" | "pathfollow" | "pathfollow3d" => {
            "A moving platform is a kinematic RigidBody whose Transform game code changes every tick; characters standing on it are carried. wish://docs/recipes has one to copy (\"A moving platform\")."
        }
        "checkpoint" | "respawn" | "spawnpoint" | "killzone" | "deathzone" => {
            "Checkpoints and respawning are game code: a sensor Collider and CollisionStarted, and a fall line. wish://docs/recipes has them (\"Falling off and checkpoints\")."
        }
        "light" | "pointlight" | "spotlight" | "arealight" | "sunlight" => {
            "Lights: DirectionalLight (the sun: parallel rays along the entity's -Z axis, illuminance_lux), PointLight (a bulb: intensity_lm, range) and SpotLight (a cone along -Z: intensity_lm, range, inner_angle_deg, outer_angle_deg). Fill light is the Ambient resource; capture_frame with view lights shows where each light reaches."
        }
        "position" | "location" | "rotation" | "orientation" | "scale" | "localtransform"
        | "worldtransform" | "transform3d" | "node3d" => {
            "Placement is the Transform component: translation (meters), rotation (a quaternion; in edits also \"euler_deg\": [x, y, z]) and scale. GlobalTransform is the derived world placement."
        }
        "color" | "colour" | "albedo" | "texture" | "texture2d" | "textures" | "skin" | "tile"
        | "tiling" | "uv" | "normalmap" | "bumpmap" | "decal" | "standardmaterial"
        | "standardmaterial3d" | "meshmaterial" | "materialoverride" | "shader"
        | "shadermaterial" => {
            "Surface looks are the Material component: base_color [r, g, b, a], metallic, roughness, emissive for a plain look. Textures are in a material file: asset_create with a .mat.json path and material { base_color_texture: a PNG of the project, normal_texture, uv_scale (tiling), alpha_mode Mask (cut-outs), double_sided, ... } (schema_get MaterialFile), then name the file in the entity's Material.file. Every entity that names it shares the look; on a Model it replaces the model's materials. There are no custom shaders. wish://docs/recipes has \"A texture on a box\"."
        }
        "transparency" | "transparent" | "opacity" | "alpha" | "glass" | "translucent"
        | "translucency" | "seethrough" | "fade" | "fader" | "canvasgroup" | "blend" => {
            "See-through surfaces are the Material component: a base_color alpha below 1 blends the surface over what is behind it (0.5 half, 0 invisible; game code can change it every tick to fade something). In a material file it is alpha_mode Blend (glTF BLEND materials are see-through too). They are lit and fogged like any surface, cast no shadow and are drawn farthest first. wish://docs/recipes has \"A glass window\"."
        }
        "script" | "behaviour" | "behavior" | "monobehaviour" | "component" | "controller" => {
            "Behaviour is game code: Rust systems in scripts/src/lib.rs built with script_build (see wish://docs/sdk). Scenes hold data only."
        }
        "postprocess" | "postprocessing" | "postprocessvolume" | "globalvolume"
        | "worldenvironment" | "lookpreset" | "lightingpreset" => {
            "The look of a scene is resources, not a component: Ambient (source Sky), Sky (sun_size_deg), Fog, Bloom, ColorGrading and RenderSettings (antialiasing, shadow_cascades), each set with set_resource. edit_apply's { \"op\": \"apply_look\", \"look\": \"natural\" } (or plain, vivid) sets all of them at once."
        }
        "sky" | "skybox" | "environment" | "fog" => {
            "The sky is the Sky resource (zenith, horizon, ground colors and luminance), set with set_resource; there is no skybox component."
        }
        _ => return None,
    })
}

/// The quoted names in a message: `'Collider'` in "There is no component
/// called 'Collider'.".
pub fn quoted_names(message: &str) -> Vec<&str> {
    message.split('\'').skip(1).step_by(2).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn familiar_names_are_explained_and_others_are_not() {
        assert!(explain("BoxCollider").unwrap().starts_with("Physics"));
        assert!(explain("NAME").unwrap().contains("id"));
        assert!(explain("MeshRenderer").unwrap().contains("Primitive"));
        assert!(explain("Texture").unwrap().contains("base_color_texture"));
        assert!(explain("Skin").unwrap().contains("Material.file"));
        assert!(explain("Tiling").unwrap().contains("uv_scale"));
        assert!(explain("Banana").is_none());
        assert_eq!(
            quoted_names("There is no component called 'Collider'."),
            ["Collider"]
        );
        assert_eq!(quoted_names("'a' and 'b'"), ["a", "b"]);
    }
}
