# ADR 0026 — Driver audio di serie, con l'uscita consegnata «appena possibile»

- **Stato**: accettata dall'utente il 26 novembre 2026, **in prova fino alla campagna di 24 ore** del passo P1.10. Le misure sono nei sottopassi P1.8a–P1.8d della [guida](../fasi/fase-1/P1.8-il-driver-audio.md); l'adozione nell'immagine è il sottopasso P1.8e. **Modificata il 27 novembre 2026** (sottopasso P1.8f): la patch vale solo per i flussi a retroazione implicita; vedi la sezione in fondo.
- **Data**: 26 novembre 2026
- **Origine**: domanda Q-14 del documento 0 («un driver audio proprio?»), punti aperti A-50 e A-69, criterio 8 della [Fase 1](../fasi/fase-1/00-piano-della-fase-1.md); [ADR 0007](0007-versioni-buildroot-e-kernel.md) per la regola sulle patch fuori albero; [ADR 0021](0021-core-live-senza-kernel.md) per ciò che il driver non deve chiedere ai core live.

## Contesto

Il requisito R-01 chiede un giro, dall'ingresso all'uscita per il cavo, sotto i 10 ms. Sulla Scarlett 18i20 di seconda generazione il driver di serie del kernel (`snd-usb-audio`) lo rispetta. Sulla quarta generazione, che è l'interfaccia di riferimento dal passo P1.12, no: 10,93 ms a 44,1 kHz e 10,23 a 48 kHz, con la forma `96:3` _(misurato, P1.12c)_.

Da prima della Fase 0 il progetto teneva aperta una strada costosa: scrivere un driver audio nostro, `dawos-uac` (Q-14). Il criterio 8 della fase dice quando: solo se nessuna configurazione del driver di serie rispetta insieme R-01 e l'ADR 0021.

Il passo P1.8 ha misurato dove sta il tempo in più _(P1.8a, P1.8b)_.

- Il giro dentro l'interfaccia è la somma di tre pezzi: il buffer che il motore sceglie (6,00 ms), il tempo che un trasferimento d'uscita passa presso il controller USB (2,98 ms), e un resto dell'interfaccia (0,6 ms).
- Dei 2,98 ms, uno è la durata del trasferimento e due sono **anticipo**: il driver del controller (`xhci-hcd`) dà a ogni trasferimento il numero del frame in cui parte, e lo vuole ben dopo «adesso» _(verificato nel sorgente: `xhci_get_isoc_frame_id`)_.
- Quella regola vale solo per i trasferimenti che non portano il segno `URB_ISO_ASAP`. `snd-usb-audio` non lo mette _(verificato nel sorgente)_.
- Quei due millisecondi stanno **sotto** il driver audio: un `dawos-uac` scritto come un driver audio USB qualunque li troverebbe identici.

Una patch di una riga a `snd-usb-audio`, dietro un parametro (`iso_asap`, spento se nessuno lo chiede), mette quel segno sui trasferimenti d'uscita. Misurata sul 285K:

| Misura | Kernel di serie | Con `iso_asap` acceso |
| --- | --- | --- |
| Uscita presso il controller, 48 kHz _(P1.8c)_ | 2,98 ms | 1,10 ms |
| Giro per il cavo, al massimo in 100 avvii, 44,1 kHz _(P1.8d)_ | 10,93 ms | 9,09 ms |
| Lo stesso, 48 kHz | 10,23 ms | 8,35 ms |
| Lo stesso, 96 kHz | 9,72 ms | 7,85 ms |
| Lo stesso, 192 kHz | 9,67 ms | 7,80 ms |
| Un'ora a 96 kHz, quattro thread al 60 %, sotto la campagna _(P1.8d)_ | `PASS`, giro 908 campioni | `PASS`, giro 729 campioni all'inizio e alla fine, nessuno slittamento, nessun xrun |
| Chiamate di sistema nella fase di calcolo | 0 | 0 |

## Decisione

1. **`dawos-uac` non si scrive nella Fase 1.** Il driver audio di dawOS è `snd-usb-audio`, quello del kernel a monte, con una patch nostra di una riga. Ai fini del criterio 8 questa è «una configurazione del driver di serie»: rispetta R-01 a tutte le frequenze e l'ADR 0021 (nessuna chiamata di sistema nel calcolo, una nella consegna, al massimo due nell'attesa). La domanda Q-14 è chiusa con «no», con le riserve scritte sotto.
2. **La patch entra nell'albero** come `external/patches/linux/0001-ALSA-usb-audio-playback-URBs-as-soon-as-possible.patch`. È la prima patch fuori albero del progetto. Nel driver l'interruttore **nasce spento**: un kernel di dawOS a cui nessuno chiede nulla si comporta come quello a monte.
3. **Lo accende il profilo di avvio**, sulla riga di comando del kernel: `snd_usb_audio.iso_asap=1`, nella chiave `kernel_arguments` del profilo `285k`. Così sta nell'immagine firmata, si legge in `/proc/cmdline`, e vale dal primo flusso. Il profilo `generic` non lo accende.
4. **Si può spegnere a caldo**, senza riavviare: `echo 0 > /sys/module/snd_usb_audio/parameters/iso_asap`. Vale dal prossimo flusso che si apre. È il modo di tornare indietro se un'interfaccia o una macchina non lo regge, e il modo in cui gli strumenti confrontano i due comportamenti.
5. **Il rapporto d'avvio dice com'è**: campo `usb_asap=on|off|none`. Un parametro scritto male sulla riga di comando viene scartato dal kernel senza un messaggio; lì si vedrebbe.

## Le tre condizioni dell'ADR 0007

L'ADR 0007 ammette una patch fuori albero a tre condizioni.

- **Arriva con la misura che la giustifica.** È la tabella sopra; i dati grezzi sono in `docs/misure/fase-0/raw/` (`usb-…-p18b`, `usb-…-p18c`, `latency-…-p18c`, `hours-…-p18d`, `latency-…-p18d`).
- **Viene proposta a monte quando possibile.** Non ancora fatto, ed è una decisione dell'utente: una proposta alla lista di ALSA parte a nome di una persona. La patch è scritta per poterlo essere (un parametro, nessun cambiamento se è spento, la misura nel messaggio). A monte potrebbero preferire un'altra forma: per esempio accenderlo da soli quando il flusso è a retroazione implicita.
- **Ha un responsabile del riporto a ogni nuova LTS.** Il progetto: a ogni cambio di kernel Buildroot la applica, e la costruzione si ferma se non si applica più. Il contesto che tocca è una riga in `data_ep_set_params` (`sound/usb/endpoint.c`), più la dichiarazione del parametro.

## Alternative scartate

- **Un driver nostro (`dawos-uac`)**: settimane di lavoro, e sulla latenza nessun guadagno che la patch non dia già, perché l'anticipo è del driver del controller. Avrebbe senso solo per ragioni che oggi non ci sono (vedi «Quando si riapre»).
- **Accendere l'interruttore di fabbrica nella patch**: il kernel di dawOS si comporterebbe diversamente da quello a monte per qualunque scheda USB, senza che nulla lo dica.
- **Farlo accendere dal motore**: legherebbe `dawos-engine` a una patch del kernel e gli chiederebbe un permesso in più; e varrebbe solo mentre il motore gira.
- **Le «fette»** (un periodo del driver più corto del ciclo del motore, opzione `--slices`): con il kernel di serie quattro fette tolgono mezzo millisecondo, non due; chiedono di rifare il risveglio degli altri thread live e quadruplicano gli interrupt USB. Restano una riserva: con la patch darebbero altri 0,75 ms.
- **Cambiare la regola nel driver del controller** (`xhci-hcd`): toccherebbe ogni dispositivo USB isocrono della macchina, non solo l'audio.

## Conseguenze

- **Il cuscino è più sottile.** Con il kernel di serie l'uscita aspettava due millisecondi presso il controller: un ritardo del kernel di un millisecondo non faceva danno. Ora ogni trasferimento d'uscita viene consegnato un decimo di millisecondo prima del suo turno. In un'ora sotto carico e in quattrocento avvii non è mancato una volta; **di quanto avanzi non è ancora misurato** (sottopasso P1.8e: il bus letto sotto carico).
- **Un trasferimento consegnato tardi non è un xrun.** ALSA non lo vede. Lo vedono il conteggio che attraversa l'interfaccia e il confronto tra il giro all'inizio e alla fine: dal passo P1.8d sono giudizi di ogni ora di prova.
- **Nella prima forma valeva per ogni scheda audio USB collegata alla macchina**, ed era stato misurato solo sulla 18i20 di quarta generazione. Dal 27 novembre 2026 vale solo per i flussi a retroazione implicita (vedi la modifica in fondo).
- **Vale per il controller del 285K** (anticipo dichiarato di 4 microframe, numeri di frame contigui). Su un altro controller il guadagno e il rischio vanno rimisurati: l'interruttore sta nel profilo della macchina per questo.
- **L'uscita soltanto.** L'ingresso segue ancora la regola del numero di frame, e non costa latenza: un trasferimento d'ingresso torna quando è pieno.
- **Otto fette restano inutilizzabili** (l'ingresso perde un pacchetto su tre): non servono.
- **Il giro è più ripetibile**: da un avvio all'altro si sposta di 7 campioni a 48 kHz invece di 49, di 1 a 96 kHz invece di 97 _(P1.8d)_.

## Modifica del 27 novembre 2026: solo la retroazione implicita

Decisione dell'utente, sottopasso P1.8f. La patch mette il segno «appena possibile» solo sui trasferimenti d'uscita di un flusso **a retroazione implicita**: quello in cui ogni trasferimento d'uscita viene accodato quando torna un trasferimento d'ingresso. È il modo della Scarlett 18i20 di quarta generazione, a tutte le frequenze _(verificato: lo dice il driver, `Implicit Feedback Mode: Yes`)_, ed è l'unico in cui la patch è stata misurata.

Per le interfacce che usano gli altri modi del driver, tra cui la 18i20 di seconda generazione, il kernel di dawOS si comporta come quello a monte anche con l'interruttore acceso: restano sul percorso misurato nelle Fasi 0 e 1, che rispetta R-01 senza patch.

Scartate: ricollegare la seconda generazione e misurarla con l'interruttore acceso (un'ora di lavoro e di cavi per un guadagno che non mi aspetto); lasciare la patch valida per tutte, scrivendo che non è provata.

Conseguenza: se un giorno un'interfaccia con un altro modo avesse lo stesso anticipo di troppo, la patch andrebbe allargata, con la sua misura.

## Dal sottopasso P1.8e: il margine misurato

Sotto il carico della campagna, in due minuti per caso, la consegna più tarda di 238 000 ha consumato 45 µs dei circa 115 disponibili; nessun microframe saltato. Un microframe saltato non cambia il giro finché la memoria dell'interfaccia lo assorbe _(ragionamento, non misura)_, ma lascia il tempo dell'uscita presso il controller più lungo di 125 µs fino al riavvio del flusso: le ore di prova leggono il bus all'inizio e alla fine e ne contano la differenza (sottopasso P1.8f).

## Quando si riapre

- Se la campagna di 24 ore (P1.10), fatta con l'interruttore acceso, mostra slittamenti del conteggio o un giro che cambia.
- Se la lettura del bus sotto carico (P1.8e) mostra che il margine della consegna si consuma: allora prima un anticipo scelto da noi all'avvio del flusso, di una frazione di millisecondo, e non il ritorno ai due.
- Se una nuova LTS cambia `xhci-hcd` o `snd-usb-audio` in modo che la patch non valga più o non serva più.
- Se a monte la proposta viene accolta in un'altra forma: si adotta quella e la patch nostra si toglie.
- Per `dawos-uac`: se serve qualcosa che `snd-usb-audio` non può dare con una configurazione o una patch piccola. Oggi i candidati sono l'avvio dei flussi (A-72, A-59, A-62; sottopasso P1.8f) e la chiamata di sistema alla consegna, che l'ADR 0021 ammette.
