# Examples

Every `.msx` file here is shown by the playground, the samples page and the
docs. To add one:

1. Put the file in this directory, with a short `//` comment at the top that says
   what it shows.
2. Add an entry to `index.json` with its `file`, a `title`, a `category` from the
   list at the top of that file and a one line `description`. Add a `note` when
   the preview in the browser leaves something out (shader fills and audio need a
   native renderer). Use `id` only when the file name has characters that do not
   belong in a link.
3. Run `cargo test --test examples`. It checks that the file parses, that it
   survives the binary roundtrip and that `index.json` names it. A duplicate that
   should not be shown goes in `hidden`.

`svg/` holds the SVG sources that the animated `anim_*.msx` files were made from
with `msx import`, and the converter page offers them as examples too. After
changing an SVG there, run the import again and keep the comment header of the
`.msx` file.

The playground opens an example directly from a link such as
`/playground/#example=blend_modes`, and the builder in
`scripts/site/build_examples.py` publishes the files and a manifest next to the
page.
