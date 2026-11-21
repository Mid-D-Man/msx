# Keyframe animation

Animation in MSX is data inside the scene. A track names an element by `id`, picks one property of it, and lists keyframes. To draw a frame, the animation resolver evaluates every track at a time `t` and returns an ordinary static scene, which any renderer then draws. No renderer needs to know about keyframes.

<!-- example: orbit_pulse | animated -->

## Scene-level fields

| Key | Meaning | Default |
|---|---|---|
| `duration` | Timeline length in seconds | the latest keyframe time across all tracks |
| `loop_mode` | `once`, `loop` or `ping_pong` | `once` |
| `animations::` | The list of tracks | none |

`once` plays to the end and holds the last frame. `loop` returns to the start. `ping_pong` plays forward and then backward, forever. An unrecognized value is read as `once`.

## Tracks and keyframes

```dixscript
animations::
  { target_id = "orbit_dot", property = "translate_x",
    keyframes = [ { time = 0.0, value = -140.0 },
                  { time = 1.2, value = 140.0, easing = "ease_in_out" } ] }
```

| Field | Meaning |
|---|---|
| `target_id` | The `id` of the element to animate |
| `property` | The channel to drive, from the table below |
| `keyframes` | At least one keyframe |

Each keyframe has a `time` in seconds, a `value`, and an optional `easing`. Several tracks can target the same element, one for each property. A track needs at least one keyframe, and keyframes can be listed in any order.

Outside the first and last keyframe times, a track holds the nearest keyframe's value. Between two keyframes it interpolates from the earlier value to the later one.

## Properties

| Property | Unit | Identity | Effect |
|---|---|---|---|
| `translate_x`, `translate_y` | pixels | 0 | Moves the element |
| `scale_x`, `scale_y` | multiplier | 1 | Scales the element |
| `rotate` | degrees | 0 | Rotates the element |
| `opacity` | multiplier | 1 | Multiplies the element's own opacity |
| `z_index` | sort key | | Replaces a layer's `z_index`. Layers only |
| `fill_r`, `fill_g`, `fill_b`, `fill_a` | 0 to 255 | | Replaces one channel of the fill color of a shape |
| `stroke_r`, `stroke_g`, `stroke_b`, `stroke_a` | 0 to 255 | | Replaces one channel of the stroke color of a shape |
| `stroke_width` | pixels | | Replaces the stroke width, never below 0 |
| `dash_offset` | pixels | | Replaces the dash offset |
| `width`, `height` | pixels | | Replaces the size of a `rect`, never below 0 |
| `rx`, `ry` | pixels | | Replaces the corner radius of a `rect` or the radius of an `ellipse`, never below 0 |
| `r` | pixels | | Replaces the radius of a `circle`, never below 0 |
| `x1`, `y1`, `x2`, `y2` | pixels | | Replaces an end point of a `line` |
| `dash_array_0`, `dash_array_1` | pixels | | Replaces the first or second entry of the dash array, which becomes two entries long |

A property that has no track keeps its identity value. `z_index` is different from the others. It replaces the layer's own value and does not combine with it, because a sort key has to stay distinguishable from an untouched one.

## Geometry channels

The last eleven properties are geometry channels. They replace a value of the shape the way the style channels do, and each one works on the shapes that have the value: `width` and `height` on a `rect`, `rx` and `ry` on a `rect` or an `ellipse`, `r` on a `circle`, and `x1` to `y2` on a `line`. On any other shape the track changes nothing. Sizes and radii never go below 0, so a shape can grow from nothing.

The two dash array channels write a two entry array, a dash and a gap, which is how a spinner animates `stroke-dasharray`. An entry that has no track keeps the value of the shape, or the other entry when the shape has no array, or 0.

A track on a geometry channel makes the file [binary version 3](binary-format.md#versions). The `anim_bars`, `anim_dots_syncbase` and `anim_ring_dash` examples use them.

## Style channels

The last ten properties are style channels. Like `z_index` they replace a value, so a channel without a track keeps what the shape already has. They write to the style of the shape that the track targets:

- A color channel changes one component and keeps the other three. A color is therefore four tracks, one per channel, and a track on `fill_b` alone leaves red, green and alpha as they are. Values are rounded and held to 0 to 255.
- When the fill or the stroke is not a plain color (`none`, a gradient or pattern reference, `currentColor` or unset), the first color track turns it into a color that starts from black. Set all four channels to avoid a surprise.
- Colors interpolate per channel between the keyframes, without premultiplying the alpha. Easing applies to all channels of a track the same way, so the channels of one color stay in step when their tracks share key times and easing.
- A group is not a target. The CPU and GPU renderers do not inherit a group's style, so a color track on a group would draw differently per renderer. Target the shapes inside it.
- A track on a style channel makes the file [binary version 3](binary-format.md#versions).

```
{ target_id = "dot", property = "fill_r",
  keyframes = [ { time = 0.0, value = 255.0 }, { time = 1.0, value = 40.0 } ] }
{ target_id = "dot", property = "dash_offset",
  keyframes = [ { time = 0.0, value = 251.3 }, { time = 2.0, value = 0.0 } ] }
```

The `anim_color_cycle` and `anim_dash_draw` examples use these channels.

## Easing

The `easing` of a keyframe shapes the segment that arrives at it. The first keyframe's easing has no segment to shape, so it has no effect.

| Value | Curve |
|---|---|
| `linear` | Constant speed |
| `ease_in` | Starts slowly and accelerates |
| `ease_out` | Starts quickly and decelerates |
| `ease_in_out` | Slow at both ends, fast in the middle |

An unrecognized value is read as `linear`.

## Where the pivot is

An animated translate, scale or rotate wraps the element's static transform from the outside. It acts in the coordinate space of the element's parent, so a rotation turns the element about the parent's origin and not about the element's own center. A scale also grows the element's distance from that origin.

The `orbit_pulse` example shows this. Its `wobble_box` sits at (40, 40) and rotates by 30 degrees, so the box swings along an arc around the canvas corner.

To turn an element about its own center, draw the element around the origin and give an enclosing group the position.

```dixscript
{ type = "group", transform = "translate(200, 120)",
  elements = [
    { type = "rect", id = "spinner", x = -30, y = -30, width = 60, height = 60,
      style = { fill = #f5a623 } }
  ] }
```

A `rotate` track on `spinner` now turns the square about the origin of the group, which is the square's center. The group carries the position and is not animated.

## What each element responds to

| Element | Responds to |
|---|---|
| `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path`, `text` | transform channels, `opacity` and the style channels |
| `image` | transform channels and `opacity` |
| `group` | transform channels, and `opacity` multiplies the group's style opacity. The style channels are ignored |
| `layer` | transform channels, `opacity` and `z_index` |
| `use` | transform channels |
| `sdf` | transform channels |
| `splat` | `translate` moves the center, `scale` multiplies the sigmas, `rotate` adds to the rotation, and `opacity` multiplies |

Children of a group or a layer can be targets too, wherever they sit in the tree. An element without an `id` cannot be animated, and a `target_id` that matches nothing has no effect.

## Playing an animation

- The [playground](/playground/) plays any scene with keyframe tracks and shows a play bar with a position slider. Editing the source while it plays keeps it playing from the same position. The animated examples in its dropdown are marked with a triangle.
- `msx animate` samples the timeline on the CPU and writes a looping GIF.
- `msx animate-gpu` does the same on the GPU. It also advances the shader `time` uniform, so keyframes and shaders animate together.
- `msx-viewer` plays a keyframed scene live in a native window.

The playground draws SVG, so it shows keyframe animation but not shader fills. The shader clock is separate from the keyframe timeline. A scene with a shader and no keyframes has no timeline of its own, so `animate-gpu` needs an explicit `--duration` for it.

A compiled file keeps its tracks. Values are stored as 32 bit floats, so `msx roundtrip` compares them with a small tolerance instead of exactly.
