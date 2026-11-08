## Context

Audit hiện tại cho thấy theme ExoRoute có 7 lựa chọn và catalog/token nền tốt, nhưng theme chưa nhất quán xuyên suốt giao diện; hành vi “theo hệ thống” cũng bị mất sau lần khởi chạy đầu. Bạn đã chọn giữ nguyên 7 visual theme hiện tại, **bổ sung lựa chọn System rõ ràng**, và sửa toàn bộ nhóm vấn đề trong cùng một đợt mà không redesign lại concept.

Worktree tracked hiện sạch; có `.zcode/` untracked do harness, không đụng tới.

## Điểm yếu đã xác nhận

1. **System preference không còn theo OS sau lần đầu.** `getStoredTheme()` trả scheme theo `prefers-color-scheme`, nhưng `App.svelte` gọi `applyTheme()` ngay khi mount; hàm này lưu theme đã resolve vào `localStorage`. Listener sau đó chỉ chạy nếu chưa có key, nên system fallback tự biến thành lựa chọn cố định. Hiện UI cũng chưa có lựa chọn System.
2. **First paint có thể không đúng preference.** Bootstrap trong `web/index.html` bọc cả logic trong một `try`; nếu localStorage lỗi, script thoát trước khi áp dụng `matchMedia`. Bootstrap cũng lặp danh sách theme/dark IDs, và đặt `theme-color` theo hai màu chung thay vì `metaColor` riêng từng theme.
3. **Không đồng bộ nhiều tab.** Không có `storage` listener cho `exoroute.theme`.
4. **Radio group chưa thực sự accessible bằng keyboard.** `AppearancePanel.svelte` khai báo radio roles nhưng cả 7 button đều là tab stop riêng; không có arrow-key navigation/roving tabindex.
5. **CSS chỉ nhất quán ở một số bề mặt.** Dark shell, mobile topbar/sidebar, Ark portal/popovers/tooltips, modal, docs và chart frame/control còn kế thừa nhiều màu/viền/bóng kiểu classic hoặc hard-coded. `docs.css` có khối Neo-Brutalist áp dụng rộng và chỉ có override generic dark; theme archetypes không phủ docs. Chart marks dùng tokens khá tốt nhưng chart-card/tooltip/control chưa theo cùng hệ.
6. **Một số màu hỗ trợ có nguy cơ không đủ tương phản.** Các `--text-dim` của vài dark palette cần được đo, không thể chỉ tin swatch/visual impression. Hiện chưa có contrast gate.
7. **Localization thiếu chất lượng dù key parity pass.** Appearance/Theme, mô tả picker, và mô tả 7 theme hiện vẫn là tiếng Anh trong 9 locale (es, pt, ja, id, ko, de, fr, hi, ar); parity test hiện chỉ kiểm tra key/value không rỗng, không phát hiện bản dịch y hệt tiếng Anh.
8. **Test coverage quá mỏng.** `theme-styles.test.ts` chủ yếu kiểm tra catalog và `applyTheme()` cho 2 theme; chưa bao phủ preference/System lifecycle, bootstrap parity, storage failure, UI keyboard, locale thực sự được dịch, CSS coverage hay contrast.

Các màu trạng thái semantic (success/error/warning) và nhận diện thương hiệu cần được giữ có chủ đích; không đổi màu hàng loạt chỉ vì phát hiện hex literals.

## Kế hoạch triển khai đề xuất

### 1. Tách “lựa chọn người dùng” khỏi “theme đang render”

**Files:** `web/src/lib/theme.ts`, `web/src/App.svelte`, `web/index.html`, các caller của quick toggle.

- Giữ `THEMES` là 7 visual theme; thêm `ThemePreference = ThemeId | 'system'` riêng. `system` không phải palette, không được truyền nhầm như `ThemeDefinition`.
- Thêm các hàm typed như `getStoredThemePreference()`, `resolveTheme(preference, systemScheme)` và cập nhật `applyTheme()` để nhận **theme đã resolve** nhưng chỉ persist khi người dùng chủ động chọn; không ghi giá trị `light`/`dark` khi preference vẫn là `system`.
- Khi không có preference hợp lệ, chọn `system`; theme effective thay đổi theo `matchMedia`. User-selected theme cũ trong `exoroute.theme` vẫn được đọc như trước, không cần xóa/migrate dữ liệu cũ.
- Bổ sung lựa chọn System trong Appearance UI. Khi quick toggle được kích hoạt từ System, chuyển thành lựa chọn classic Light/Dark ngược với scheme đang hiển thị — hành vi rõ ràng, không tự ghi đè system trong nền.
- Thêm `storage` event để đổi theme ở tab khác cập nhật preference, root attributes, meta color và Svelte state. Event không được tạo vòng lặp ghi storage.
- Sửa bootstrap để storage lỗi vẫn resolve OS preference; cập nhật `theme-color` theo catalog cho cả first paint và runtime. Giữ bootstrap đồng bộ với TypeScript catalog bằng parity test, tránh ID/dark-ID drift.

### 2. Làm accessibility và localization hoàn chỉnh

**Files:** `AppearancePanel.svelte`, 12 catalog trong `web/src/lib/locales/`, `web/tests/i18n-catalog-parity.test.ts`, test mới cho appearance UI.

- Triển khai radiogroup đúng pattern: một tab stop (roving tabindex), Arrow keys di chuyển/chọn, Home/End theo convention, Space/Enter activation; giữ visible focus ring, checked state và accessible name ngắn, ổn định.
- Thêm System option có mô tả effective scheme hiện tại; đảm bảo responsive grid và touch target, thêm `prefers-reduced-motion` cho hover/selection animation.
- Dịch Appearance labels, mô tả theme, style tag/description theo chiến lược i18n hiện có; giữ nguyên proper nouns. Nâng test để phát hiện English-identical values trong các key được yêu cầu dịch thay vì chỉ kiểm tra key parity.
- Không gắn nhãn toàn bộ locale là đã được dịch chỉ vì key có mặt; translation parity và translation quality là hai tiêu chí riêng.

### 3. Chuẩn hóa color/surface token contract

**Files:** `tokens.css`, `palettes.css`, `theme-base.css`, `theme-components.css`, `theme-settings-dialogs.css`, `theme-styles.css`, `ark-components.css`, `base.css`.

- Mở rộng shared tokens cho shell/topbar/sidebar, raised surface, modal, popover/tooltip, backdrop, border/shadow và semantic text levels.
- Chuyển các shell/modal/portal/scrollbar/background hard-coded colors sang token tương ứng; đảm bảo portal/popover không phụ thuộc DOM nesting để kế thừa theme.
- Thêm override có chủ đích cho cả 7 theme (light/dark scheme và palette), đặc biệt Tokyo Night, Catppuccin, Linear, Cyber Neon, Minimal Light; không thêm hàng loạt `!important` để thắng cascade.
- Đo contrast cho `ink`, `muted`, `text-dim`, accent, focus và trạng thái ở kích thước thực. Điều chỉnh palette token không đạt WCAG AA cho text thường, tối thiểu 4.5:1; 3:1 cho large text/non-text UI theo tiêu chuẩn phù hợp.
- Giữ success/error/warning colors ổn định khi chúng mang nghĩa semantic; document rule này và kiểm tra contrast trên đúng bề mặt.

### 4. Đồng bộ các bề mặt còn lệch archetype

**Files:** `docs.css`, `features/overview/ChartFrame.svelte`, `TrafficSeries.svelte`, `BreakdownChart.svelte`, `RankedBars.svelte`, modal và logo components.

- Thay docs-wide Neo-Brutalist styling bằng tokens/archetype selectors có phạm vi rõ; docs sidebar/topbar/cards/buttons/code panels phải theo theme mà không mất affordance.
- Chuyển chart-card, dimension switches, tooltip và warning badge sang surface/semantic tokens; rà soát tooltip trong cả SVG và portaled UI.
- Quy định logo có cố định brand accent hay theo palette; thống nhất comment, variant, và usage thay vì chỉ đổi SVG theo cảm quan.
- Rà soát font remote/theme typography để fallback không gây layout shift đáng kể; giữ `prefers-reduced-motion` cho theme/card transitions.

### 5. Regression suite và QA

**Tests:** mở rộng `theme-styles.test.ts`, thêm test `theme-preference.test.ts`, Appearance UI tests và token/contrast tests.

- Test preference matrix: không có key → System; key theme cũ → giữ nguyên; `system` → resolve theo OS; invalid id → fallback; localStorage get/set throw → vẫn render; OS change cập nhật chỉ khi System; explicit choice không bị OS change ghi đè.
- Test storage event: valid preference áp dụng ở tab khác, invalid/removed key đưa về System, không phát sinh write loop.
- Test catalog ↔ pre-paint bootstrap parity cho IDs, scheme và meta colors.
- Test Appearance UI keyboard semantics, selection, visible focus, reduced-motion rule và accessible labels.
- Test 7 theme × representative shell, modal, portal popover, docs, chart; kiểm tra contrast các token semantic theo bảng surfaces. Giữ test CSS selector-focused vừa đủ, kết hợp browser QA thay vì giả định jsdom chứng minh giao diện trực quan.
- Chạy browser visual QA trên các theme light/dark tiêu biểu ở desktop/mobile; sau đó rà đủ 7 theme ở shell, Settings → Appearance, modal/popover, docs và overview charts.

## Verification

```powershell
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check --workspace --all-targets --all-features
cargo check --release
node scripts/check_source_boundaries.mjs
```

Từ `web/`:
```powershell
npm run check
npm test
npm run build
```

Ngoài unit/UI test, xác nhận first paint khi localStorage bị chặn, thay đổi OS theme trong khi app đang mở, chọn theme ở tab khác, và keyboard-only thao tác radio group. Không tuyên bố tương phản/coverage đạt nếu test đo hoặc browser QA không xác nhận.