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

---

## 3. Modificações Implementadas

1. `crates/rnote-ui/src/app/appactions.rs`:
   - Adicionado `<Meta>` nos aceleradores de `app.quit` e `app.new-window`.
   - Adicionado o método `setup_macos_menubar(&self)` configurando os menus File, Edit (Undo, Redo, Cut, Copy, Paste, Select All, Clear Document), View e Window (`gtk-macos-special="window-submenu"`).

2. `crates/rnote-ui/src/app/mod.rs`:
   - Invocado `obj.setup_macos_menubar()` no `startup(&self)`.

3. `crates/rnote-ui/src/appwindow/actions.rs`:
   - Atualizada a lista de aceleradores em `setup_action_accels` adicionando `<Meta>` em todas as ações (`win.undo`, `win.redo`, `win.clipboard-*`, `win.save-doc`, etc.).

4. `crates/rnote-ui/data/style-mac.css`:
   - Corrigido o padding do headerbar (`padding-left: 78px;`).

5. **Deploy:**
   - Compilado via GitHub Actions (Run ID `41752017042`).
   - Binário e recursos instalados em `/Applications/Rnote.app`.
