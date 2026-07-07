# sl-anim

Pure Second Life / OpenSim **animation** decoding: the Linden keyframe-motion
binary format (`.anim`) that a viewer plays to pose an avatar's skeleton.

Like its siblings `sl-mesh` (LLMesh), `sl-texture` (J2C), and `sl-avatar`
(skeleton / base body) the crate is deliberately **Bevy-free and I/O-free**: it
decodes a borrowed `&[u8]` into an owned [`Motion`] and never opens a file or
fetches from the grid. Resolving an animation UUID to its bytes (a built-in
viewer asset or an uploaded `.anim` fetched over `ViewerAsset`) and driving a
skeleton from the decoded tracks live in the runtime / `sl-client-bevy` layers,
at the I/O and entity boundaries.

A `.anim` file is a base-priority + duration + loop / ease / hand-pose header
followed by a list of animated joints, each carrying quantised rotation and
position keyframe tracks, and an optional list of collision-volume constraints.
The angles and times are stored as `u16` quantised values (rotations as a
three-component imaginary quaternion in `[-1, 1]`, positions in
`[-5, 5]` metres, times as a fraction of the motion's duration), all
little-endian, decoded here into `f32` in Second Life's right-handed **Z-up**
metre space.

The pieces are:

- `decode` — the keyframe-motion binary decoder and its owned model (`Motion`,
  `JointMotion`, `RotationKey`, `PositionKey`, `Constraint`, and the priority /
  hand-pose / constraint enums).

Both the modern `1.0` encoding and the legacy `0.1` encoding are decoded — the
latter (times as `f32` seconds, rotations as `f32` Euler angles built with the
reference viewer's `mayaQ`, positions clamped to `[-5, 5]`) still backs many
decades-old Second Life animation assets that visual updates never replace.

The binary layout follows Firestorm `LLKeyframeMotion::deserialize`
(`indra/llcharacter/llkeyframemotion.cpp`, read-only reference; reimplemented
here idiomatically).
