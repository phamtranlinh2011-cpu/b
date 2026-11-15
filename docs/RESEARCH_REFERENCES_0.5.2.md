# NOVA Native 0.5.2 — UI research references

1. [Microsoft Learn — Customize the Windows 11 Start layout](https://learn.microsoft.com/en-us/windows-hardware/customize/desktop/customize-the-windows-11-start-menu): Windows Start is organized into Pinned, Recommended and All sections; the pinned subset is a grid and All apps has category/grid/list views.
2. [Microsoft Support — Customize the Windows Start Menu](https://support.microsoft.com/en-us/windows/experience/personalization/customize-the-windows-start-menu): search, pins, recommendations, grouping and personal organization.
3. [Microsoft Learn — Windows app development best practices](https://learn.microsoft.com/en-us/windows/apps/get-started/best-practices): Windows 11 design principles; Mica for long-lived surfaces and Acrylic for transient flyouts.
4. [egui 0.37.0 — Area](https://docs.rs/egui/0.37.0/egui/containers/struct.Area.html): anchored floating surfaces and foreground ordering.
5. [egui Context — animation APIs](https://docs.rs/egui/0.37.0/egui/struct.Context.html): `animate_bool_with_time` and stable ID-based animation.
6. [egui Painter — drawing APIs](https://docs.rs/egui/0.37.0/egui/struct.Painter.html): text, filled rectangles and strokes for custom shortcut interactions.

NOVA does not inject into or replace the Windows Explorer shell. The design is an in-app, Windows-inspired desktop surface built inside the existing eframe viewport.
