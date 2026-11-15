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
        | "meshfilter" | "staticmesh" | "sprite" => {
            "What is drawn comes from Primitive (built-in Cube, Sphere, Plane) or Model (a glTF asset); there is no Mesh or Visibility component. To hide something remove its Primitive or Model; describe_view says what is visible."
        }
        "collider" | "boxcollider" | "spherecollider" | "meshcollider" | "rigidbody" | "body"
        | "physics" | "physicsbody" | "characterbody" | "kinematicbody" => {
            "Physics (colliders, rigid bodies) is not in this engine version yet (it comes with game systems, M6); scenes are placed with Transform only."
        }
        "light" | "pointlight" | "spotlight" | "arealight" | "sunlight" => {
            "The light component is DirectionalLight (parallel rays like the sun, along the entity's -Z axis); point and spot lights come later. Fill light is the Ambient resource."
        }
        "position" | "location" | "rotation" | "orientation" | "scale" | "localtransform"
        | "worldtransform" | "transform3d" | "node3d" => {
            "Placement is the Transform component: translation (meters), rotation (a quaternion; in edits also \"euler_deg\": [x, y, z]) and scale. GlobalTransform is the derived world placement."
        }
        "color" | "colour" | "albedo" | "texture" | "shader" => {
            "Surface looks are the Material component: base_color [r, g, b, a], metallic, roughness, emissive; textures come with Model assets."
        }
        "script" | "behaviour" | "behavior" | "monobehaviour" | "component" | "controller" => {
            "Game code (scripts) comes in a later engine version (M5). Today scenes hold data only; sim_start runs the systems the project's example provides."
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
        assert!(explain("Collider").unwrap().contains("Physics"));
        assert!(explain("NAME").unwrap().contains("id"));
        assert!(explain("MeshRenderer").unwrap().contains("Primitive"));
        assert!(explain("Banana").is_none());
        assert_eq!(
            quoted_names("There is no component called 'Collider'."),
            ["Collider"]
        );
        assert_eq!(quoted_names("'a' and 'b'"), ["a", "b"]);
    }
}
