// Builds the comparison's starting scene in a Unity project (M7): the same
// objects as Wish Engine's start (tools/compare/tasks.py, START), with Z
// mirrored because Unity is left-handed. Copy into Assets/Editor/ and run:
//   Unity.exe -batchmode -projectPath <project> -executeMethod CompareSetup.Build -quit
using UnityEditor;
using UnityEditor.SceneManagement;
using UnityEngine;

public static class CompareSetup
{
    static GameObject Make(PrimitiveType shape, string name, Vector3 at, Vector3 scale, bool collider)
    {
        var go = GameObject.CreatePrimitive(shape);
        go.name = name;
        go.transform.position = new Vector3(at.x, at.y, -at.z);
        go.transform.localScale = scale;
        if (!collider)
        {
            Object.DestroyImmediate(go.GetComponent<Collider>());
        }
        return go;
    }

    public static void Build()
    {
        var scene = EditorSceneManager.NewScene(NewSceneSetup.DefaultGameObjects, NewSceneMode.Single);
        var camera = Camera.main.gameObject;
        camera.transform.position = new Vector3(0, 1, -10);
        camera.transform.rotation = Quaternion.identity;
        Make(PrimitiveType.Cube, "floor", new Vector3(0, -0.1f, 0), new Vector3(20, 0.2f, 20), true);
        Make(PrimitiveType.Cube, "barrel", new Vector3(-3, 0.5f, 0), Vector3.one, false);
        Make(PrimitiveType.Cube, "box", new Vector3(0, 4, 0), Vector3.one, true);
        Make(PrimitiveType.Cube, "car", new Vector3(5, 0.5f, 0), Vector3.one, false);
        Make(PrimitiveType.Sphere, "wheel", new Vector3(5.5f, 0.3f, -0.6f), Vector3.one * 0.6f, false);
        Make(PrimitiveType.Cube, "old", new Vector3(-6, 0.5f, -3), Vector3.one, false);
        Make(PrimitiveType.Sphere, "target", new Vector3(0, 0.5f, 25), Vector3.one, false);
        System.IO.Directory.CreateDirectory("Assets/Scenes");
        EditorSceneManager.SaveScene(scene, "Assets/Scenes/CompareBase.unity");
        // Text scenes, so the checker can read them.
        EditorSettings.serializationMode = SerializationMode.ForceText;
        AssetDatabase.SaveAssets();
    }
}
