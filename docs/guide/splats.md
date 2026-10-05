# Gaussian splats

A `splat` is a soft elliptical blob. Its opacity is highest at the center and falls away with a Gaussian curve, so it has no hard edge. Overlapping splats composite over each other in document order, which makes them useful for glows, light, fog and atmosphere.

<!-- example: splat_atmosphere | nosource -->

```dixscript
{ type = "splat", x = 300, y = 200, sigma_x = 60, sigma_y = 30,
  rotation = 0.4, color = #4a9eff, opacity = 0.9 }
```

## Fields

| Field | Meaning | Default |
|---|---|---|
| `x`, `y` | Center | required |
| `sigma_x` | Standard deviation along the splat's own x axis, in pixels | required |
| `sigma_y` | Standard deviation along its y axis | the value of `sigma_x`, a circle |
| `rotation` | Rotation of the ellipse in radians | 0 |
| `color` | Peak color | white |
| `fill` | A paint reference that takes priority over `color` | none |
| `opacity` | Peak opacity, 0 to 1 | 1 |
| `id` | For animation tracks and `use` | none |

A splat has no `transform` field. Its position, size and rotation are fully described by the fields above.

## How big a splat is

A larger sigma gives a wider blob. By 2.4 sigma from the center the opacity has fallen to a few percent, and the SVG renderer uses that radius when it approximates the falloff.

## Fill

`fill` takes a reference such as `"url(#aurora)"`. It overrides `color`, and a scene can keep `color` set as a fallback. A gradient fill is reduced to one flat color, its stop average, and a shader fill runs on the GPU renderer. The `splat_gradient_fill` and `splat_shader_fill` examples show both.

## Where it renders

The CPU and GPU renderers evaluate the Gaussian per pixel and composite the splats in order. The SVG renderer approximates a splat with a three stop radial gradient. That is a static preview and not the real falloff.

For animation, a track on a splat changes its position, size, rotation and opacity: translate moves `x` and `y`, scale multiplies the sigmas, rotate adds to `rotation` after conversion from degrees, and `opacity` multiplies the splat's own opacity.
