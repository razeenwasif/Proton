//! 3MF: the 3D Manufacturing Format's core, what slicers read — a zip
//! holding the model as XML, each body an object of shared points and the
//! triangles between them, in millimetres.

use super::{mm, zip};
use crate::mesh::Mesh;
use crate::vec::Vec3;

const CONTENT_TYPES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
  <Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
  <Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>
</Types>
"#;

const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Target="/3D/3dmodel.model" Id="rel0" Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>
</Relationships>
"#;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The model's XML: one object for each `(name, mesh)`.
pub fn model(title: &str, bodies: &[(&str, &Mesh)]) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str("<model unit=\"millimeter\" xml:lang=\"en-US\" xmlns=\"http://schemas.microsoft.com/3dmanufacturing/core/2015/02\">\n");
    x.push_str(&format!("  <metadata name=\"Title\">{}</metadata>\n  <metadata name=\"Application\">Proton</metadata>\n  <resources>\n", escape(title)));
    for (k, (name, m)) in bodies.iter().enumerate() {
        let (pts, tris) = m.compact();
        x.push_str(&format!("    <object id=\"{}\" type=\"model\" name=\"{}\">\n      <mesh>\n        <vertices>\n", k + 1, escape(name)));
        for p in pts {
            x.push_str(&format!("          <vertex x=\"{}\" y=\"{}\" z=\"{}\"/>\n", mm(p.x), mm(p.y), mm(p.z)));
        }
        x.push_str("        </vertices>\n        <triangles>\n");
        for t in tris {
            x.push_str(&format!("          <triangle v1=\"{}\" v2=\"{}\" v3=\"{}\"/>\n", t[0], t[1], t[2]));
        }
        x.push_str("        </triangles>\n      </mesh>\n    </object>\n");
    }
    x.push_str("  </resources>\n  <build>\n");
    for k in 0..bodies.len() {
        x.push_str(&format!("    <item objectid=\"{}\"/>\n", k + 1));
    }
    x.push_str("  </build>\n</model>\n");
    x
}

/// A 3MF package of the bodies.
pub fn write(title: &str, bodies: &[(&str, &Mesh)]) -> Vec<u8> {
    let m = model(title, bodies);
    zip::write(&[("[Content_Types].xml", CONTENT_TYPES.as_bytes()), ("_rels/.rels", RELS.as_bytes()), ("3D/3dmodel.model", m.as_bytes())])
}

/// A body read back from a 3MF Proton wrote: its name, points and
/// triangles. Enough to check a file by; not a reader of every 3MF.
pub type Object = (String, Vec<Vec3>, Vec<[u32; 3]>);

pub fn read(bytes: &[u8]) -> Result<(String, Vec<Object>), String> {
    let entries = zip::read(bytes)?;
    let find = |n: &str| entries.iter().find(|(name, _)| name == n).map(|(_, d)| String::from_utf8_lossy(d).into_owned());
    let rels = find("_rels/.rels").ok_or("no _rels/.rels")?;
    if !rels.contains("/3D/3dmodel.model") {
        return Err("the package does not point at its model".into());
    }
    find("[Content_Types].xml").ok_or("no content types")?;
    let xml = find("3D/3dmodel.model").ok_or("no model")?;
    if !xml.contains("unit=\"millimeter\"") {
        return Err("not in millimetres".into());
    }
    let attr = |tag: &str, key: &str| -> Option<String> {
        let at = tag.find(&format!(" {key}=\""))? + key.len() + 3;
        Some(tag[at..at + tag[at..].find('"')?].to_string())
    };
    let title = xml.split("<metadata name=\"Title\">").nth(1).and_then(|s| s.split('<').next()).unwrap_or("").to_string();
    let mut objects = Vec::new();
    for obj in xml.split("<object ").skip(1) {
        let head = &obj[..obj.find('>').ok_or("a broken object")?];
        let name = attr(&format!(" {head}"), "name").unwrap_or_default();
        let mut pts = Vec::new();
        for v in obj.split("<vertex ").skip(1) {
            let tag = format!(" {}", &v[..v.find("/>").ok_or("a broken vertex")?]);
            let c = |k: &str| attr(&tag, k).and_then(|s| s.parse::<f64>().ok()).ok_or(format!("a vertex without {k}"));
            pts.push(Vec3::new(c("x")?, c("y")?, c("z")?));
        }
        let mut tris = Vec::new();
        for t in obj.split("<triangle ").skip(1) {
            let tag = format!(" {}", &t[..t.find("/>").ok_or("a broken triangle")?]);
            let c = |k: &str| attr(&tag, k).and_then(|s| s.parse::<u32>().ok()).ok_or(format!("a triangle without {k}"));
            tris.push([c("v1")?, c("v2")?, c("v3")?]);
        }
        objects.push((name, pts, tris));
    }
    Ok((title, objects))
}
