# `.pen` (Pencil) field mapping

What the importer reads from a `.pen` document and what it becomes in desigma. The source is
the TypeScript schema at the end of Pencil's "The .pen format" developer page (schema version
`2.21`) plus real files written by versions `2.8` to `2.13`. Pencil's documentation says the
format may change without notice, so unknown types and properties are skipped and listed in
`Import::warnings` instead of failing the import.

Status: **yes** is mapped, **partial** is mapped with a stated difference, **no** is skipped
(with a warning when it changes what is drawn).

Desigma is not affiliated with Pencil. This file describes file compatibility only.

## File

| Item | Status | Notes |
|---|---|---|
| Container | yes | Plain UTF-8 JSON, with `//` and `/* */` comments and trailing commas allowed (the docs say pen.dev accepts comments). Real files checked are plain JSON. Pencil's docs do not describe any encryption; input that is not JSON (binary, encrypted, zip, a leading BOM aside) fails with an error that says so. |
| `version` | partial | Not required. A missing, older (major below 2) or newer (above `2.21`) version is only a warning. Differences between 2.8 and 2.21 that were seen are handled, see Strokes. |
| `children` | yes | Top-level objects go on one page, "Page 1", at their `x`/`y`. |
| `variables`, `themes` | yes | See Variables and themes. |
| `imports` | no | Libraries (`.lib.pen`) are not loaded. Instances of their components become empty dashed frames named "Missing component ...", and one warning is added. |

## Objects (`type`)

| Type | Becomes | Status |
|---|---|---|
| `frame` | Frame (Component when `reusable`), with auto layout | yes |
| `group` | Group, fitted to its children | yes |
| `rectangle` | Rectangle (Component frame when `reusable`) | yes |
| `ellipse` | Ellipse with arc data | yes |
| `polygon` | Polygon (regular, `polygonCount` sides, default 3) | yes |
| `path` | Vector | yes |
| `text` | Text | yes |
| `note`, `prompt`, `context` | Text box with the object's size, named Note / Prompt / Context | partial (AI annotation objects have no counterpart; `model` is dropped) |
| `icon` | Rectangle named `library/icon` with a dashed outline in the icon's fill color | partial (icon fonts are not bundled) |
| `script` | Empty clipping frame with the script's size, name and fills | no (scripts are not run; the generated shapes are missing) |
| `browser` | Empty clipping frame with the object's size | no (live pages are not shown) |
| `ref` | Instance (Component when it also has `reusable`) | yes |
| anything else | Layer of unknown type, sized from `width`/`height` | no |

## Properties every object has

| Property | Status | Notes |
|---|---|---|
| `id` | yes | Kept as `Node::source_id`. Used to match instance nodes with their component's. |
| `name` | yes | Unnamed text layers are named after their first line, other layers after their type. |
| `x`, `y` | yes | Relative to the parent's top-left. Ignored where the parent lays children out (as the format says), except for `layoutPosition: "absolute"` children. |
| `rotation` | yes | Degrees counter-clockwise around the top-left corner, as the schema says. |
| `flipX`, `flipY` | yes | Mirrors the node inside its box (after layout, so hugging sizes are right). |
| `opacity` | yes | A variable is resolved for the theme in effect (desigma opacity bindings are percentages, so the binding is not kept). |
| `enabled` | yes | Becomes visibility; a boolean variable becomes a `visible` binding. |
| `layoutPosition` | yes | `absolute` becomes an absolutely positioned child of an auto-layout frame. |
| `theme` | yes | Selects variable modes for the subtree. |
| `reusable` | yes | See Components. |
| `context`, `metadata` | no | Notes for AI agents and tool data. |
| constraints | n/a | `.pen` has no constraints; every node keeps desigma's default (top-left). |

## Layout and size

| Property | Status | Notes |
|---|---|---|
| `layout` | yes | `horizontal` is the default when absent (as documented), `vertical`, `none` (children placed by `x`/`y`, no auto layout). |
| `gap` | yes | Item spacing. A number variable becomes an `itemSpacing` binding. |
| `padding` | yes | One value, `[vertical, horizontal]` or `[top, right, bottom, left]` (three values are read as top, horizontal, bottom). Variables become `padding*` bindings. |
| `justifyContent` | partial | `start`, `center`, `end`, `space_between`. `space_around` is drawn as `space_between` (warning). |
| `alignItems` | yes | `start`, `center`, `end`. |
| `layoutIncludeStroke` | yes | |
| `width`, `height` | yes | A number (or number variable, bound), `fill_container[(fallback)]` (Fill in a layout; elsewhere the parent's size, else the fallback), `fit_content[(fallback)]` (Hug; the fallback is the size of an empty frame, or of a frame without layout). Frames default to `fit_content`. |
| `clip` | yes | Clips content. |
| `slot` | no | Slots only suggest components in Pencil's UI. Replacing children through `descendants` works. |
| `placeholder` | no | |

Sizes the file leaves implicit are computed by desigma's layout engine after conversion
(three rounds of layout and text re-measuring, so wrapped text and hugging parents settle).

## Shapes

| Property | Status | Notes |
|---|---|---|
| `cornerRadius` | yes | One value or `[top-left, top-right, bottom-right, bottom-left]`; variables become radius bindings. Radii larger than half the box are clamped by the renderer (the docs use 9999 for pills). |
| `innerRadius`, `startAngle`, `sweepAngle` (ellipse) | yes | Ring ratio and arc. Pencil angles run counter-clockwise from the right, desigma's clockwise. |
| `polygonCount` | yes | |
| `geometry`, `viewBox`, `fillRule` (path) | yes | SVG path data (all commands, arcs become curves). The view box (default: the geometry's tight bounds) is scaled onto the node's `width`/`height`; stroke widths are not scaled. |

## Fills

`fill` is one fill or a list, painted bottom to top. Fills keep their index, so a color
variable on the Nth fill is bound as `fills/N`. Text uses the same list for its color.

| Fill | Status | Notes |
|---|---|---|
| `"#RGB"`, `"#RGBA"`, `"#RRGGBB"`, `"#RRGGBBAA"`, `"$variable"` | yes | |
| `{type: "color"}` | yes | `enabled`, `blendMode`. |
| `{type: "gradient"}` linear / radial / angular | yes | `center`, `size`, `rotation`, `colors`, `opacity`. Mapped onto desigma's node-normalized gradient handles. Linear gradients run toward `rotation` degrees counter-clockwise from up (0 up, 90 left, 180 down) with `size.height` as the length; radial uses `size` as the ellipse's diameters; angular starts at the top and runs clockwise. The direction convention is not spelled out in the docs and was checked against the real files only by plausibility. |
| `{type: "image"}` | partial | `url` as a `data:` URI or a file below the `.pen`'s folder (needs `Options::base_dir`), PNG, JPEG, GIF, WebP. `cover` is Fill, `contain` is Fit, `stretch` is drawn as Fill (warning). `transform` (cropping) is ignored (warning). An image that cannot be read is gray (warning). Remote URLs are not fetched. |
| `{type: "mesh_gradient"}` | partial | Drawn as the average of its colors (warning). |
| `{type: "shader"}` | no | Dropped (warning). |
| `enabled: false` | yes | Kept as a hidden paint. |
| `blendMode` | yes | `light` is Lighten. |

## Strokes

| Property | Status | Notes |
|---|---|---|
| `stroke` | yes | Fills as above (several allowed). |
| `strokeWidth` | yes | A number, or `{top, right, bottom, left}` (missing sides are 0). Default 1. |
| `strokeAlignment` | yes | `inner`, `center`, `outer`. Default: inside for shapes, center for paths and text. |
| `strokeLinecap`, `strokeLinejoin` | yes | `miter` is the default join. |
| Older spelling | yes | Files from versions 2.8 to 2.13 write `stroke: {align, thickness, join, cap, fill}`, with `inside` / `outside` for the alignment. Both spellings are read. |
| dashes, miter limit | no | Not in the schema. |

## Effects

`effect` is one effect or a list, applied in order.

| Effect | Becomes | Status |
|---|---|---|
| `shadow`, `shadowType: "outer"` | Drop shadow (`offset`, `blur` as radius, `color`; spread 0) | yes |
| `shadow`, `shadowType: "inner"` | Inner shadow | yes |
| `blur` | Layer blur | yes |
| `background_blur` | Background blur | yes |
| `shader` | | no (dropped, warning) |
| `enabled: false`, shadow `blendMode` | hidden / ignored | partial |

## Text

| Property | Status | Notes |
|---|---|---|
| `content` | yes | A string variable becomes a `characters` binding. |
| `fontFamily`, `fontWeight`, `fontStyle` | yes | Weight as a number or a word (`normal`, `bold`, `semibold`, ...). Variables are resolved for the theme in effect (no desigma binding exists for these). Fonts that are not installed fall back at render time. |
| `fontSize`, `letterSpacing` | yes | Pixels. Number variables are bound. |
| `lineHeight` | yes | Multiplier of the font size, becomes a percentage. Absent: the font's own. |
| `textAlign`, `textAlignVertical` | yes | `justify` is Justified. |
| `underline`, `strikethrough` | partial | Underline wins when both are set. |
| `textGrowth` | yes | `auto` (the default; `width`/`height` are ignored, as documented), `fixed-width` (wraps, height follows), `fixed-width-height`. |
| `fill` | yes | Solid and gradient text fills. |
| `stroke`, `effect` | yes | |
| `href` | no | Links are dropped. |

## Components and instances

| Feature | Status | Notes |
|---|---|---|
| `reusable: true` | yes | A frame or rectangle becomes a Component; a group becomes a Component with its children; any other object gets a Component frame of its own size around it. The component stays where the file put it. Components can be used before they are defined and can be nested. |
| `type: "ref"` | yes | An Instance linked to its Component: every node of the copy points at the node it mirrors (matched by `.pen` id), and fields that differ are recorded as overrides, so desigma's reset and sync behave. Properties on the `ref` object override the component root's. |
| `descendants`, property overrides | yes | By id path (`"ok-button/label"`), also through nested instances. |
| `descendants`, replacement (entry has a `type`) | yes | The replacement is a new subtree (not linked to anything). |
| `descendants`, replaced `children` | yes | |
| `ref` with `reusable` | yes | A component derived from another (the instance is expanded and becomes a Component). |
| Missing or circular components, non-reusable targets | partial | Empty dashed frame named "Missing component ..." (warning). |
| Instances of imported libraries | no | See `imports`. |
| Component properties (variants, boolean/text/swap props) | n/a | `.pen` has none. |
| Limits | | Chains of instances deeper than 24, and more than 250 000 objects (counting what instances expand to, configurable with `Options::max_objects`), are cut off with a warning. |

## Variables and themes

| Feature | Status | Notes |
|---|---|---|
| `variables` of type `color`, `number`, `string`, `boolean` | yes | Become desigma variables named as in the file. Variables are listed alphabetically (JSON objects are unordered). |
| Values per theme | yes | The last entry whose `theme` is satisfied wins, as documented. A variable that varies along no theme goes in the collection "Variables" (one mode, "Default"); along one axis, in a collection named after the axis with a mode per axis value; along several axes, in a collection `a + b` whose modes are the combinations. |
| `themes` | yes | The first value of every axis is the default. An axis used by variables but missing from `themes` is inferred. At most 64 modes per collection. |
| `theme` on an object | yes | Sets the matching mode in each collection for that object and everything below it (ancestors' themes are merged in). |
| Variable references (`$name`) | yes | Bound where desigma can bind: fill and stroke colors, width, height, corner radii, gap, padding, stroke widths, font size, letter spacing, `enabled`, `content`. Elsewhere the value for the theme in effect is used. A variable whose value is another `$variable` becomes an alias. |
| Bound values | | The value stored on a bound property is the default theme's; the mode in effect picks the real one when drawn. |

## Warnings

`Import::warnings` lists, first seen first, anything that was skipped or approximated.
Repeats are folded ("... (x11)"). They name the kind of problem and not the object, so a
list stays short for large files.
