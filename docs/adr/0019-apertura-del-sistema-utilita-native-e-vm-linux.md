# ADR 0019 — Apertura del sistema: utilità native e VM Linux

- **Stato**: accettata dall'utente il 18 novembre 2026. Nell'immagine c'è solo la predisposizione del kernel: dal sotto-passo P1.0 (20 novembre 2026) KVM, vhost, `tun` e nftables sono dentro il kernel, non più moduli (punto 3); il dominio `vm` e tutto il resto entrano quando la macchina virtuale entra nel prodotto
- **Data**: 18 novembre 2026
- **Origine**: documento di progetto, **ADR-033** (nel repository la numerazione è un'altra: vedi la [tabella di corrispondenza](README.md)); nota dell'utente del 18 novembre 2026, punti 1, 2 e 3; misure del sotto-passo [P0.6e](../fasi/fase-0/P0.6e-chiusura-dei-punti-aperti.md), parte 1

## Contesto

dawOS nasce come sistema in cui la DAW è l'unico programma. Chi lavora con una DAW ha però bisogno anche d'altro: un browser, un lettore di documenti, i programmi dei produttori di hardware e di plugin. Ogni programma in più che gira sull'host è un rischio per il tempo reale, e un programma arbitrario è un rischio che non si può misurare in anticipo.

Il kernel può già ospitare una macchina virtuale: KVM, vhost, `tun` e la traduzione degli indirizzi in nftables sono nel kernel come moduli dal sotto-passo P0.6e, su richiesta dell'utente. Lì sono state misurate due cose _(verificato sul 285K)_:

- con i moduli caricati il rumore sui core live non cambia;
- **il caricamento** manda circa 4200 interrupt brevi a ogni core live, perché riscrive codice del kernel su tutti i core.

## Decisione

**La DAW resta l'unico programma con accesso al tempo reale, non l'unico strumento.** Intorno ci sono utilità native e una macchina virtuale Linux leggera per il browser e i programmi installabili. Niente Windows, per ora.

### 1. Utilità native

Moduli come gli altri, nell'immagine.

- **Musicali**: accordatore; metronomo con _tap tempo_; misuratore di loudness (LUFS, _true peak_); analizzatore di spettro; generatore di segnale e test di latenza; convertitore _batch_ di file audio; browser di campioni; notazione di base (MusicXML in ingresso e in uscita, MIDI in partitura); editor di testi e accordi.
- **Di gestione**: note e file di testo; lettore PDF; gestione dei file; «Installa plugin» ([ADR 0020](0020-plugin-linux-di-terze-parti.md)).

### 2. La macchina virtuale

- **Che cosa la esegue**: QEMU con KVM, come processo senza privilegi con `-sandbox on`, in un gruppo di controllo dedicato.
- **Quante**: una per utente, con il disco nella home cifrata.
- **Sistema ospite**: un'immagine Linux minimale fornita da dawOS, firmata e aggiornata in blocco con il sistema. I programmi dell'utente si installano come Flatpak (Flathub) nel disco dell'utente.
- **Risorse predefinite**: 2 vCPU fissate sui core 18–19 (punto 5); 4 GiB di memoria su pagine da 2 MiB; nessuno swap sull'host, zram nell'ospite; 64 GiB di disco; peso basso per l'I/O. L'utente può salire fino a 4 core e 8 GiB.
- **Grafica**: virtio-gpu paravirtualizzata. Le finestre dei programmi compaiono nel compositor di dawOS come finestre native, tramite un proxy Wayland nell'ospite (contesto _cross-domain_). Nessun desktop dell'ospite.
- **Lanciatore**: un agente nell'ospite comunica a dawOS le applicazioni installate (nomi, icone) e le avvia su richiesta, tramite vsock.
- **File**: la macchina virtuale vede la cartella dell'utente con virtiofs, mai le cartelle di sistema. Scambio e Documenti in lettura e scrittura; Progetti e Libreria in sola lettura; sempre escluse le cartelle dei plugin, le configurazioni nascoste e lo spazio dell'agente.
- **Rete**: NAT con mascheramento in nftables. La macchina virtuale non raggiunge i servizi di dawOS: socket di `dawosd`, MCP, tunnel degli aggiornamenti.
- **USB**: chiavette e telefoni con usbredir. L'interfaccia audio e i controller MIDI restano sempre a dawOS.
- **Agente AI**: non vede e non comanda la macchina virtuale.

### 3. Avvio

- **KVM e vhost sono pronti dall'avvio del sistema, prima del motore** (nota dell'utente, punto 2). Caricarli quando la macchina virtuale parte manderebbe 4200 interrupt a ogni core live mentre il motore è già attivo.
- **La macchina virtuale parte dopo l'accesso dell'utente**, in background, quando la DAW è pronta e l'autoverifica di avvio è finita. Nel profilo Live non parte.
- **Come: dentro il kernel** (scelta dell'utente del 19 novembre 2026, realizzata nel sotto-passo [P1.0](../fasi/fase-1/P1.0-raccordo.md)). KVM per Intel, `vhost-net`, `vhost-vsock`, `tun`, nftables con `nat` e `masquerade` non sono più moduli: non c'è nulla da caricare né da ordinare rispetto a `dawos-tune`, e `/dev/kvm` esiste dal primo istante. L'esclusione di `kvm_intel` dal caricamento automatico e lo strumento `dawos-vm-modules` sono stati tolti. Il rapporto di avvio ha il campo `kvm=1` quando il dispositivo c'è.

### 4. Modalità di lavoro

| Modalità | Quando | Macchina virtuale | Bus audio della macchina virtuale |
| --- | --- | --- | --- |
| Ferma | Trasporto fermo, profilo diverso da Live | Piena | Collegato |
| Playback | Trasporto in riproduzione | Quota di CPU dimezzata (`cpu.max`) | Staccato |
| Recording | Trasporto in registrazione | Congelata (`cgroup.freeze`); il motore attende la conferma prima di registrare | Staccato |
| Live | Profilo Live, per tutta la sessione | Non avviata | Staccato |

### 5. I core della macchina virtuale

- **Core 18–19, tolti al dominio `system`** (nota dell'utente, punto 3). Nel profilo `285k`, quando la macchina virtuale entra: un dominio `vm` sui core 18–19, e il dominio `system` passa a 16–17.
- Il gruppo di controllo della macchina virtuale ha `cpuset.cpus` = 18–19: vi stanno i thread delle vCPU, i thread di I/O di QEMU, i worker vhost e `virtiofsd`.
- **Nella Fase 0 il profilo non cambia**: questa decisione è solo scritta, finché la campagna non è chiusa.

### 6. Audio

**La macchina virtuale non tocca mai ALSA.**

- **Dall'ospite a dawOS**: uscita dell'ospite su virtio-snd, poi QEMU, poi un ponte `dawos-vmaudio` fuori dal tempo reale, sui core di servizio, con un ricampionatore asincrono; da lì un buffer circolare senza lock porta al bus d'ingresso «VM» del motore. Buffer vuoto vuol dire silenzio, senza attesa.
- **Da dawOS all'ospite**: un bus di mandata porta l'uscita della DAW, o un ingresso della scheda, all'ingresso virtio-snd dell'ospite.
- Entrambi i versi sono staccati fuori dalla modalità Ferma.

### 7. Criteri di accettazione, quando la macchina virtuale entra

- **Rumore e ritardo di risveglio**: `dawos-osnoise` e `dawos-wakeup` sui core live, con la macchina virtuale sotto carico in modalità Ferma, danno gli stessi numeri che senza.
- **Ciclo di vita**: avvio, congelamento, scongelamento e arresto della macchina virtuale, misurati sui core live con un carico real-time attivo, non devono mandare interrupt ai core live. È la regola del sotto-passo P0.6e sulla riscrittura del codice del kernel.
- **Grafica accelerata e rischio K-16**: il rendering dell'ospite passa per il driver `xe` dell'host e moltiplica le mappature di memoria grafica, quindi le pulizie che avvisano tutti i core. Da misurare. Ripiego: rendering software nell'ospite.
- **_Cache Allocation_ (RDT)**: verificare `cat_l3` sul 285K; se c'è, una porzione fissa della cache L3 alla macchina virtuale.

## Alternative scartate

Il confronto completo sta nel documento di progetto. Qui le alternative su cui il repository ha misure, o che la nota del 18 novembre 2026 nomina.

| Alternativa | Perché no |
| --- | --- |
| Caricare i moduli KVM quando la macchina virtuale parte | Il caricamento manda circa 4200 interrupt a ogni core live _(verificato, P0.6e)_, e la macchina virtuale parte quando il motore è già attivo. |
| Dare alla macchina virtuale due E-core del dominio anticipativo | Il dominio scenderebbe da circa 9,15 a 8,0 core live equivalenti, e la sessione di riferimento a 96 kHz salirebbe al 76 %, sopra la soglia del 75 % ([ADR 0011](0011-r04-carico-sintetico.md)) _(stima dell'utente sui numeri dell'esperimento E-06)_. |
| Darle due E-core del dominio `io` | Sono già la riserva per R-04 (ADR 0011). |
| Una macchina virtuale Windows | Fuori dal perimetro, per ora. |
| Un desktop dell'ospite | Le finestre dei programmi stanno nel compositor di dawOS, una per una. |
| Lasciare che l'ospite usi la scheda audio | L'interfaccia audio è del motore, sempre; l'ospite passa da un ponte fuori dal tempo reale. |

## Conseguenze

- **Che cosa c'è già** _(verificato sulla configurazione del kernel 6.18.64)_: KVM per Intel, `vhost-net`, `vhost-vsock`, `tun`, nftables con `nat` e `masquerade`, come moduli (dal sotto-passo P1.0: dentro il kernel); le pagine grandi (`HUGETLBFS`); i gruppi di controllo con `cpuset`. Il processore dichiara `vmx`, `ept` e le pagine da 1 GiB _(verificato in `docs/misure/fase-0/topologia-285k.txt`)_.
- **Che cosa manca nell'immagine**: QEMU, `virtiofsd`, il programma `nft`, usbredir, l'immagine dell'ospite, l'agente, il ponte audio, il compositor. Nel kernel manca `NFT_CT` (un filtro che guarda lo stato della connessione; il NAT non ne ha bisogno) e manca l'interfaccia per la _Cache Allocation_ (`X86_CPU_RESCTRL`).
- **_Cache Allocation_: la risposta c'è già, ed è probabilmente no.** Tra i flag del processore registrati per il 285K c'è `rdt_a` ma non ci sono né `cat_l3` né `cat_l2` _(verificato nello stesso file; il kernel mostra quei flag anche senza `X86_CPU_RESCTRL`, verificato nei sorgenti)_. I flag sono quelli del core di avvio, un P-core: resta da confermare con l'interfaccia vera, quando il kernel la avrà.
- **Il dominio `system` si dimezza**: da quattro E-core a due, per i servizi del sistema, `sshd`, il registro e, domani, `dawosd`. Nessuna misura dice oggi se bastano _(da verificare quando il profilo cambia)_.
- **I moduli all'avvio cambiano la configurazione misurata** (dal sotto-passo P1.0 non sono più moduli: vale lo stesso per il codice dentro il kernel). La campagna P0.7 certifica la macchina senza moduli. Dopo la chiusura di P0.8, sull'immagine con i moduli caricati all'avvio, si rifanno `dawos-hwnoise` e `dawos-osnoise` per dieci minuti e il piano `short` della campagna (10 minuti e 1 ora di `timerlat`, 1 ora di `cyclictest`), da confrontare con la parte 1 di [P0.7](../fasi/fase-0/P0.7-campagna-di-misura.md). Nello stesso sotto-passo si ricontrolla la caduta del collegamento di rete vista una volta dopo un caricamento (punto aperto A-36).
- **Il congelamento in registrazione** ferma l'ospite senza avvisarlo: al risveglio il suo orologio salta e una connessione di rete aperta può essere scaduta _(da verificare)_. È il prezzo scelto perché in registrazione l'ospite non consumi nulla.

Punti che la decisione lascia da precisare, e che ho segnato nel [registro dei punti aperti](../02-punti-aperti.md) (A-45):

1. **Dove stanno il terzo e il quarto core** quando l'utente sale a 4: il dominio `vm` ne ha due, e i ragionamenti del punto 5 escludono sia il dominio anticipativo sia il dominio `io`.
2. **«Core di servizio»** (ponte audio) non è un dominio del profilo: lo leggo come dominio `system`. Da confermare.
3. **Le pagine da 2 MiB** vanno riservate: all'avvio, quando la memoria è ancora compatta, oppure alla partenza della macchina virtuale, con il rischio di non trovarle. Nel primo caso sono 4 GiB tolti al sistema anche nel profilo Live, dove la macchina virtuale non parte.

## Quando si riapre

- Se le misure del punto 7 falliscono: la macchina virtuale in modalità Ferma disturba i core live, o il suo ciclo di vita manda loro interrupt.
- Se il rendering accelerato dell'ospite porta rumore sui core live e il ripiego software non basta ai programmi previsti.
- Se due E-core non bastano al dominio `system`.
- Se si decide di supportare Windows nell'ospite.
- Con la seconda macchina supportata: i core della macchina virtuale sono un dato del profilo.
