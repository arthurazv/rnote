# Rnote macOS Development Context & Roadmap

Este documento consolida o diagnóstico técnico completo, os arquivos alterados e o guia para implementar as soluções de atalhos e interface do Rnote no macOS.

## 1. Informações do Ambiente e Repositório
- **Caminho Local:** `/Users/arthurazv/.gemini/antigravity/scratch/rnote`
- **Repositório Git:** `git@github.com:arthurazv/rnote.git` (branch `main`)
- **Instalação Atual:** `/Applications/Rnote.app`
- **Compilação:** GitHub Actions (`.github/workflows/build_macos.yml`) rodando em runner `macos-14` (Apple Silicon).

---

## 2. Diagnóstico Técnico dos Problemas de Atalhos e Menus no macOS

### A. Atalhos do Teclado (`⌘Z`, `⌘C`, `⌘V`, `⌘S`, etc.) não respondem
1. **Diferença de modificadores GTK 3 vs GTK 4:**
   - No GTK 3, `<Primary>` mapeava automaticamente para Command (`⌘`) no macOS.
   - No GTK 4, `<Primary>` é estritamente um alias para `<Control>` (`⌃`) em todas as plataformas.
   - No macOS (backend Quartz do GTK 4), a tecla Command (`⌘`) é mapeada exclusivamente pelo modificador **`<Meta>`**.
   - Como o Rnote usava `&["<Primary>z", "<Ctrl>z"]`, o GTK só escutava `Ctrl+Z`, ignorando a tecla `Command`.

### B. Menu "Edit" Nativo do macOS com itens cinzas (desabilitados)
1. Sem um menubar explícito definido via `GtkApplication::set_menubar()`, o GTK 4 cria um menu padrão usando as ações de campos de texto nativos (`text.undo`, `text.redo`, etc.).
2. No Rnote, a área de notas é um canvas vetorial proprietário (não é um `GtkText`), portanto essas ações do GTK ficam desativadas.
3. No AppKit do macOS, quando um item de menu padrão está desabilitado, o sistema operacional consome o atalho de teclado (`⌘Z`, `⌘C`, etc.) e nem sequer entrega o evento para a janela do aplicativo.
4. **Solução:** Configurar um menu nativo via `app.set_menubar()` associando diretamente às ações reais do Rnote (`win.undo`, `win.redo`, `win.clipboard-*`, `win.new-tab`, etc.).

### C. Espaçamento dos botões da janela (Traffic Lights)
- Ajuste necessário em `crates/rnote-ui/data/style-mac.css` adicionando `padding-left: 78px;` no seletor `window:not(.fullscreen) headerbar` para evitar sobreposição dos botões fechar/minimizar/expandir.

### D. Otimização de Fluidez no Pinch-to-Zoom (Trackpad)
- Eliminado o reflow de layout e re-alocação de memória a cada evento de pinça a 120Hz.
- `appwindow/mod.rs`: `zoomed_temporarily` agora aciona diretamente `canvas.queue_draw()`, aproveitando a matriz de transformação do GSK via GPU (Metal) sem sobrecarregar a thread principal.
- `canvas/mod.rs`: removida chamada redundante de `queue_resize()` em `configure_adjustments()`.
- `canvaswrapper.rs`: suporte a `Cmd + Scroll` adicionando `SUPER_MASK` e `META_MASK`, e `queue_resize()` apenas no encerramento do gesto (`connect_end`/`connect_cancel`).

### E. Integração Nativa com o Sistema de Arquivos do macOS
- **Filtros de Extensão (`NSSavePanel` / `NSOpenPanel`):** No backend macOS do GTK 4, o diálogo de arquivos mapeia para as janelas nativas do AppKit, que ignoram MIME types de Linux (`application/rnote`, `image/svg+xml`). Ajustados todos os seletores em `dialogs/export.rs` e `dialogs/import.rs` para utilizar `filter.add_pattern("*.ext")` e `filter.add_suffix("ext")`.
- **Abertura Multi-Arquivo no Finder:** Em `app/mod.rs`, implementada iteração de `files` no evento `open` do `GtkApplication`, permitindo selecionar múltiplos documentos `.rnote` no Finder ou abri-los via CLI em abas individuais simultâneas.
- **Histórico Nativo (Open Recent):** Integração com `gtk4::RecentManager` em `appwindow/mod.rs` e `canvas/imexport.rs` ao abrir ou salvar documentos.
- **Associação de Documentos e Ícones Retina Apple:**
  - Gerados ícones de alta resolução Apple Retina (`Rnote.icns` e `RnoteDoc.icns` em `build-aux/macos/`) a partir dos vetores SVG originais via `rsvg-convert` e `iconutil`.
  - Configurado `Info.plist` com `CFBundleDocumentTypes`, `LSHandlerRank` como `Owner` e `UTExportedTypeDeclarations` para o UTI `com.github.flxzt.rnote.document`.
  - Registrado automaticamente com o LaunchServices do macOS (`lsregister`).

---

## 3. Modificações Implementadas

1. `crates/rnote-ui/src/app/appactions.rs`:
   - Adicionado `<Meta>` nos aceleradores de `app.quit` e `app.new-window`.
   - Adicionado o método `setup_macos_menubar(&self)` configurando os menus File, Edit (Undo, Redo, Cut, Copy, Paste, Select All, Clear Document), View e Window (`gtk-macos-special="window-submenu"`).

2. `crates/rnote-ui/src/app/mod.rs`:
   - Invocado `obj.setup_macos_menubar()` no `startup(&self)`.
   - Suporte a múltiplos arquivos no método `open(&self, files: &[gio::File], hint: &str)`.

3. `crates/rnote-ui/src/appwindow/actions.rs`:
   - Atualizada a lista de aceleradores em `setup_action_accels` adicionando `<Meta>` em todas as ações (`win.undo`, `win.redo`, `win.clipboard-*`, `win.save-doc`, etc.).

4. `crates/rnote-ui/src/appwindow/mod.rs` & `crates/rnote-ui/src/canvas/mod.rs`:
   - Otimização de fluidez no zoom e pinch-to-zoom (evitando `queue_resize()` contínuo em 120Hz).
   - Registro de arquivos abertos no `gtk4::RecentManager`.

5. `crates/rnote-ui/src/canvas/imexport.rs`:
   - Registro de arquivos salvos no `gtk4::RecentManager`.

6. `crates/rnote-ui/src/dialogs/export.rs` & `crates/rnote-ui/src/dialogs/import.rs`:
   - Filtros de arquivos compatíveis com o AppKit nativo do macOS (`*.rnote`, `*.svg`, `*.pdf`, `*.png`, etc.).

7. `crates/rnote-ui/data/style-mac.css`:
   - Corrigido o padding do headerbar (`padding-left: 78px;`).

8. `build-aux/macos/`:
   - `Rnote.icns` e `RnoteDoc.icns` nativos Retina.
   - Script gerador `generate_icons.py`.
   - Script de compilação e empacotamento local: `build_local.sh --install`.

9. `crates/rnote-ui/src/filetype.rs`:
   - Suporte a Apple UTIs (`com.adobe.pdf`, `public.png`, `public.jpeg`, `public.svg-image`, `com.github.flxzt.rnote.document`).
   - Fallback de extensão insensível a maiúsculas/minúsculas e extração via URI/basename.

10. `crates/rnote-ui/src/canvas/mod.rs` & `crates/rnote-ui/src/appwindow/actions.rs`:
    - Suporte a `gdk::FileList`, `gdk::Texture` e URIs `file://` no Drag & Drop do Finder.
    - Suporte a colar imagens (`gdk::Texture`) e arquivos copiados (`gdk::FileList`) da área de transferência via `⌘V`.

11. **Deploy & Compilação Local:**
    - Ferramental nativo instalado no Mac: `rustc`, `cargo`, `meson`, `ninja`, `cmake`, `appstream`.
    - Executável compilado nativamente e ativo em `/Applications/Rnote.app`.
