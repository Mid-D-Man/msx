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

A property that has no track keeps its identity value. `z_index` is different from the others. It replaces the layer's own value and does not combine with it, because a sort key has to stay distinguishable from an untouched one.

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
| `rect`, `circle`, `ellipse`, `line`, `polyline`, `polygon`, `path`, `text`, `image` | transform channels and `opacity` |
| `group` | transform channels, and `opacity` multiplies the group's style opacity |
| `layer` | transform channels, `opacity` and `z_index` |
| `use` | transform channels |
| `sdf` | transform channels |
| `splat` | `translate` moves the center, `scale` multiplies the sigmas, `rotate` adds to the rotation, and `opacity` multiplies |

Children of a group or a layer can be targets too, wherever they sit in the tree. An element without an `id` cannot be animated, and a `target_id` that matches nothing has no effect.

## Playing an animation

- The [playground](/playground/) plays any scene with keyframe tracks and shows a play bar with a position slider. Editing the source while it plays keeps it playing from the same position.
- `msx animate` samples the timeline on the CPU and writes a looping GIF.
- `msx animate-gpu` does the same on the GPU. It also advances the shader `time` uniform, so keyframes and shaders animate together.
- `msx-viewer` plays a keyframed scene live in a native window.

The playground draws SVG, so it shows keyframe animation but not shader fills. The shader clock is separate from the keyframe timeline. A scene with a shader and no keyframes has no timeline of its own, so `animate-gpu` needs an explicit `--duration` for it.

A compiled file keeps its tracks. Values are stored as 32 bit floats, so `msx roundtrip` compares them with a small tolerance instead of exactly.
