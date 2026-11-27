# DS Buddy

App para Windows que encontra e pareia controles **DualShock 4** e **DualSense** por Bluetooth, mesmo com dongles teimosos (como os clones CSR8510, `VID_0A12&PID_0001`). O "DS" vale para os dois: **D**ual**S**hock e **D**ual**S**ense.

## Por que existe

Com alguns adaptadores, o controle em modo de pareamento nunca aparece na tela **Adicionar dispositivo** do Windows, ou aparece e falha na primeira tentativa. A tela de Configurações desiste rápido. O DS Buddy usa a API clássica de Bluetooth do Windows e insiste:

1. Faz buscas seguidas (~8 s cada) até achar um controle PlayStation, pelo nome ou pela classe de dispositivo "gamepad".
2. Chama o controle direto pelo endereço (`BluetoothAuthenticateDeviceEx`), sem depender da lista do Windows.
3. Se falhar, tenta de novo a cada 3 s, por até 10 minutos.
4. No sucesso, ativa o serviço HID do controle.

Não precisa de administrador.

## Uso

**Parear:** coloque o controle em modo de pareamento e clique em **Procurar e parear**. Para isso, segure **Create** (DualSense) ou **Share** (DualShock 4) junto com **PS** até a luz piscar duas vezes seguidas, várias vezes. Se o Windows mostrar uma janela ou notificação pedindo confirmação, aceite. O quadro **Atividade** mostra cada busca e cada tentativa.

**Controles:** lista os controles que o Windows conhece.
- **Reparear** esquece o pareamento antigo e pareia de novo. Use quando o controle não conecta mais, por exemplo depois de usá-lo no PS5. Coloque-o em modo de pareamento antes.
- **Esquecer** remove o controle do Windows. Pede um segundo clique para confirmar.

Para reconectar um controle já pareado, basta apertar o botão PS.

## Compilar

```
cargo build --release
```

O executável fica em `target\release\ds-buddy.exe`. A interface é em [Slint](https://slint.dev) com renderização por software, então não precisa de GPU nem de WebView2.

- `ui\app.slint` e `ui\widgets.slint`: a interface.
- `src\bt.rs`: chamadas à API de Bluetooth do Windows.
- `src\pairing.rs`: o fluxo de busca e pareamento, que roda numa thread separada.
- `scripts\make-icon.ps1`: gera `ui\icon.png` e `icon.ico`.

Para tirar capturas de tela, `DS_BUDDY_PAGE=1` abre o app direto na aba Controles.

## Licença

GPL-3.0-or-later (veja `LICENSE`). A interface usa o Slint sob a GPLv3.

As fontes IBM Plex são distribuídas sob a SIL Open Font License (`ui\fonts\LICENSE.txt`).
