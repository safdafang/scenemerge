//! Semantic model of Godot 4 scenes and resources, and identity matching
//! between two versions of a file.
//!
//! [`SceneFile`] gives a typed view of a [`godot_text::Document`]: the file
//! descriptor, external and internal resources, the node tree, signal
//! connections and editable-instance markers. [`Matching`] decides which
//! entities of two versions are "the same thing", independently of local id
//! renumbering and section order.
//!
//! ```
//! use scene_model::{Matching, MatchOptions, SceneFile};
//!
//! let a = SceneFile::parse(r#"[gd_scene format=3]
//!
//! [ext_resource type="Script" path="res://p.gd" id="1_aaaaa"]
//!
//! [node name="Player" type="Node2D"]
//! script = ExtResource("1_aaaaa")
//! "#)?;
//! // Same scene, but the editor renumbered the ext_resource id.
//! let b = SceneFile::parse(r#"[gd_scene format=3]
//!
//! [ext_resource type="Script" path="res://p.gd" id="2_bbbbb"]
//!
//! [node name="Player" type="Node2D"]
//! script = ExtResource("2_bbbbb")
//! "#)?;
//! let m = Matching::compute(&a, &b, &MatchOptions::default());
//! assert_eq!(m.ext_a_to_b, vec![Some(0)]);
//! let (pa, pb) = (&a.nodes[0].props[0], &b.nodes[0].props[0]);
//! assert!(m.values_equal(&pa.value, &pb.value, 0.0));
//! # Ok::<(), scene_model::ModelError>(())
//! ```

mod error;
mod identity;
mod model;

pub use error::ModelError;
pub use identity::{MatchHow, MatchOptions, Matching, Side};
pub use model::{
    join_path, Connection, Descriptor, Editable, ExtResource, FileKind, Issue, Node, ParentRef,
    Prop, SceneFile, SubResource, DEFAULT_CONNECTION_FLAGS, MAX_FORMAT,
};
