# NOVA Native 0.5.3 — Performance references

1. [egui Ui::request_repaint_after](https://docs.rs/egui/latest/egui/struct.Ui.html): schedule a delayed repaint rather than continuously requesting immediate frames; the docs call out saving battery and avoiding wasted redraws while idle.
2. [eframe App trait](https://docs.rs/eframe/latest/eframe/trait.App.html): UI is called when a repaint is needed, so periodic updates should be requested only for active live work.
3. [sysinfo performance tips](https://docs.rs/sysinfo/latest/sysinfo/): refresh only the information required, and reuse a single System instance; `refresh_all` retrieves process, memory and CPU data.
4. [sysinfo System source](https://docs.rs/crate/sysinfo/latest/source/src/common/system.rs): exposes narrower CPU, memory and process refresh methods.

NOVA applies these principles by throttling the visible System Monitor, limiting process enumeration frequency, and avoiding full egui style reconstruction on every frame.
