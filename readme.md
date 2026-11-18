# Taskbar Mouse Battery (Rust)

Indicador ultraleve de porcentagem de bateria na barra de tarefas do Windows 11, escrito em Rust puro usando a API nativa do Windows (`Win32` + `hidapi`).

Desenvolvido e testado para o mouse sem fio **Delux M900 Pro 8K** (`VID: 0x1D57`, `PID: 0xFA65` — receptor `LXDDZ 2.4G 8K HS Receiver`).

## Recursos

- **Ultraleve**: binário nativo em Rust sem Electron/runtime pesado, com consumo de CPU próximo de 0% e cerca de 1 MB de RAM.
- **Leitura 100% passiva (somente leitura)**: escuta os pacotes de telemetria enviados automaticamente pelo receptor na interface HID `000A:0000` (`03 50 41 01 <bateria>`), sem enviar comandos nem interferir no polling rate de 8000 Hz.
- **Estilo nativo do Windows 11**: ícone minimalista vetorial de mouse e fonte `Segoe UI Variable Display` posicionados ao lado da bandeja do sistema.
- **Ocultação automática em tela cheia**: esconde o indicador automaticamente quando jogos ou vídeos estão em fullscreen no mesmo monitor.
- **Cores de alerta de bateria**:
  - Branco: acima de 20%
  - Amarelo: entre 11% e 20%
  - Vermelho: 10% ou menos

## Requisitos

- Windows 10 ou Windows 11 (64-bit)
- [Rust / Cargo](https://rustup.rs/) (toolchain `x86_64-pc-windows-msvc`)

## Como compilar e executar

Clone o repositório e rode os comandos abaixo na pasta do projeto:

```powershell
# Testar leitura passiva no terminal (mova o mouse durante 6 segundos)
cargo run -- --diagnose

# Compilar versão final otimizada
cargo build --release

# Executar o programa
.\target\release\taskbar-mouse-battery.exe
```

## Iniciar automaticamente com o Windows

Execute no PowerShell (ajustando o caminho se você mover o `.exe`):

```powershell
$ws = New-Object -ComObject WScript.Shell
$s = $ws.CreateShortcut("$env:APPDATA\Microsoft\Windows\Start Menu\Programs\Startup\MouseBattery.lnk")
$s.TargetPath = "$PWD\target\release\taskbar-mouse-battery.exe"
$s.Save()
```

## Encerrar o programa

Como o aplicativo roda em segundo plano sem janela principal, encerre pelo Gerenciador de Tarefas (`taskbar-mouse-battery.exe`) ou pelo PowerShell:

```powershell
Stop-Process -Name taskbar-mouse-battery
```
