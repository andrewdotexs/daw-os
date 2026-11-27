# 6. Glossario

I termini in ordine alfabetico, con una riga di spiegazione e il capitolo dove sono trattati per esteso. In fondo, le sigle del progetto.

I termini inglesi sono quelli che si trovano nei manuali, nei messaggi del kernel e nelle uscite delle prove; accanto c'è la parola italiana usata nel quaderno.

## I termini

| Termine | Che cosa vuol dire | Dove |
| --- | --- | --- |
| **A/B (slot)** | Due posti sul disco per due versioni del sistema: si aggiorna quello non in uso e si torna indietro da soli se non parte | Cap. 2, La piattaforma |
| **Accelerazione** (_speed-up_) | Quante volte più in fretta finisce il ciclo su N thread rispetto a uno | Cap. 2, Il grafo |
| **ALSA** | L'interfaccia di Linux verso le schede audio | Cap. 2, La scheda audio |
| **Anello** (_ring buffer_) | Una coda di lunghezza fissa in cui uno scrive e un altro legge senza lucchetti. È dove finisce il registro dei cicli | Cap. 2, Il motore |
| **Anticipo** (del controller USB) | Quanto prima della partenza un trasferimento va consegnato al controller: 2,0–2,5 ms sul 285K con il kernel di serie, 0,1 con l'interruttore `iso_asap` acceso | Cap. 2, La scheda audio |
| **Assegnazione statica** | Ogni nodo del grafo ha il suo thread, deciso alla compilazione (D-10) | Cap. 2, Il grafo |
| **Assestamento** (_settle_) | I secondi che l'interfaccia impiega a stabilizzarsi dopo un cambio di frequenza | Cap. 2, La scheda audio |
| **Attesa attiva** (_spin_, _busy wait_) | Aspettare girando a vuoto invece di dormire, per non entrare nel kernel | Cap. 2, Il grafo |
| **AVX2, FMA** | Istruzioni che lavorano su otto numeri alla volta, e che moltiplicano e sommano in un'operazione | Cap. 2, Il calcolo |
| **Bilancio di calcolo** (_budget_) | L'istante oltre il quale un thread non comincia un altro nodo: la scadenza del ciclo meno il margine | Cap. 2, Il motore |
| **Biquad** | Sezione del secondo ordine: il mattone dei filtri digitali | Cap. 2, Il calcolo |
| **BPF classico** | Piccoli programmi che il kernel esegue per filtrare chiamate di sistema o pacchetti. Caricarne uno fa ripulire il predittore dei salti su tutti i processori (A-76) | Cap. 2, Il tempo reale |
| **Buffer** | La coda di campioni tra programma e scheda; nel grafo, il blocco di campioni all'uscita di un nodo | Cap. 2 |
| **Buildroot** | Lo strumento che costruisce l'immagine dai sorgenti | Cap. 2, La piattaforma |
| **Campagna** | Ore di misura sotto un carico che cambia a fasi | Cap. 3, La campagna |
| **Campione** (_sample_) | Il valore del segnale in un istante, per un canale | Cap. 2, L'audio come numeri |
| **Cane da guardia** (_watchdog_) | Il thread che sorveglia i thread live e sostituisce quello bloccato | Cap. 2, Il motore |
| **Catena di dipendenza** | Operazioni di cui ognuna aspetta il risultato della precedente: ne fissa il costo minimo | Cap. 2, Il calcolo |
| **Cella** | Ciò che un nodo ricorda da un ciclo all'altro; passa da un grafo al successivo | Cap. 2, Il grafo |
| **cgroup** | Gruppi di controllo: come Linux assegna processori e memoria a gruppi di processi | Cap. 2, La piattaforma |
| **Chiamata di sistema** (_syscall_) | La richiesta di un programma al kernel. Nella fase di calcolo sono vietate | Cap. 2, Il tempo reale |
| **Ciclo** | Un giro del motore: attesa, lettura, calcolo, consegna. Uno per periodo | Cap. 2, Il motore |
| **Ciclo partito indietro** (_behind_) | Un ciclo cominciato quando il suo periodo era già scaduto: il thread non ha dormito | Cap. 2, Il motore |
| **Ciclo tagliato** (_cut_, _incomplete_) | Un ciclo in cui il bilancio ha fermato il thread prima dell'ultimo nodo | Cap. 2, Il motore |
| **Conteggio** (_count_) | Lo strumento in cui ogni campione porta il proprio numero: trova il campione perso | Cap. 3, strumenti nel motore |
| **Costante del driver** | La parte della latenza che non dipende dal buffer: 3,4–3,7 ms sulla quarta generazione | Cap. 4, P1.12 |
| **Crate** | Un pezzo di codice Rust compilato per conto suo. Il motore ne ha quattro | Cap. 3, Le prove automatiche |
| **C-state (C1, C1E)** | Gli stati di riposo di un processore fermo. Sui live è ammesso solo C1 | Cap. 2, Il tempo reale |
| **dB, dBFS** | Decibel: rapporto di livelli in scala logaritmica; dBFS è rispetto al fondo scala | Cap. 2, L'audio come numeri |
| **Denormalizzati** (_denormals_) | Numeri in virgola mobile piccolissimi che il processore tratta lentamente | Cap. 2, Il calcolo |
| **Distribuzione** | Come si ripartiscono i valori di una misura: minimo, media, mediana, percentili, massimo | Cap. 3, Il registro dei cicli |
| **dm-verity** | Il controllo, blocco per blocco, che il disco di sistema non sia cambiato | Cap. 2, La piattaforma |
| **Dominio** | Un gruppo di processori con un compito: live, anticipativo, system, io | Cap. 1 |
| **`do_sync_core`** | La funzione con cui il kernel sincronizza tutti i processori quando riscrive il proprio codice | Cap. 2, Il tempo reale |
| **Durata** (_makespan_) | Quanto dura il ciclo con una certa assegnazione dei nodi ai thread | Cap. 2, Il grafo |
| **E-core, P-core** | I core efficienti e quelli veloci del 285K | Cap. 2, Il tempo reale |
| **EROFS** | Il file system compresso a sola lettura della radice | Cap. 2, La piattaforma |
| **Fair server** | La riserva di tempo del kernel per i processi normali; spenta sui processori live | Cap. 2, Il tempo reale |
| **Fase di attesa, fase di calcolo** | Le due metà del ciclo: una chiamata bloccante, poi nessun ingresso nel kernel | Cap. 2, Il tempo reale |
| **fcp-server** | Il programma che traduce i comandi della Scarlett di quarta generazione | Cap. 2, La scheda audio |
| **Fetta** (_slice_) | Un periodo del driver più corto del ciclo del motore: due, quattro o otto in un ciclo (opzione `--slices`) | Cap. 2, La scheda audio |
| **Follower** | Un thread live che non tiene la scheda e la segue attraverso un timer | Cap. 2, Il motore |
| **Forma del buffer** | Frame per periodo e periodi nel buffer, come `96:3` | Cap. 2, L'audio come numeri |
| **Frame** | I campioni di tutti i canali nello stesso istante | Cap. 2, L'audio come numeri |
| **FTZ, DAZ** | I due bit che dicono al processore di prendere per zero i numeri denormalizzati | Cap. 2, Il calcolo |
| **Giro** (_way round_, _round trip_) | Il percorso di un campione dall'uscita all'ingresso, e il tempo che ci mette | Cap. 3 |
| **Grafo** | I nodi e i loro collegamenti: ciò che il motore calcola in un ciclo | Cap. 2, Il grafo |
| **Immagine** | Il file con tutto il sistema, costruito in un colpo e scritto su uno slot | Cap. 2, La piattaforma |
| **Interruzione** (_interrupt_, IRQ) | Un evento che ferma ciò che il processore sta facendo per far girare un gestore del kernel | Cap. 2, Il tempo reale |
| **IPI** | Un'interruzione mandata da un processore a un altro | Cap. 2, Il tempo reale |
| **Isolamento** (`isolcpus`, `nohz_full`, `rcu_nocbs`) | Le opzioni del kernel che tengono il sistema lontano dai processori live | Cap. 2, Il tempo reale |
| **KVM** | La virtualizzazione fatta dal processore vero: senza, la macchina virtuale è emulata e lentissima | Cap. 2, La piattaforma |
| **Latenza** | Il tempo tra un suono che entra e lo stesso suono che esce; anche, per un thread, il ritardo di risveglio | Cap. 2 |
| **Lavoro** (_work_) | La somma dei costi di tutti i nodi: il ciclo su un thread solo | Cap. 2, Il grafo |
| **Live** | Il dominio dei processori riservati al ciclo audio, e i thread che ci girano | Cap. 1 |
| **Loopback** | Un'uscita riportata su un ingresso, con un cavo o dentro l'interfaccia, per misurare | Cap. 2, La scheda audio |
| **Lucchetto** (_lock_) | Ciò che impedisce a due thread di toccare la stessa cosa insieme. Nel grafo sono solo tentati, mai attesi | Cap. 2, Il grafo |
| **Margine** | Quanto prima della scadenza un thread smette di prendere nodi: 150 µs | Cap. 2, Il motore |
| **Mediana** | Il valore sotto cui sta metà delle misure | Cap. 3, Il registro dei cicli |
| **Memoria dell'interfaccia** | La piccola coda di campioni che l'interfaccia tiene tra il bus USB e il convertitore. Assorbe un ritardo sul bus senza cambiare la latenza, finché non si svuota | Cap. 2, La scheda audio; cap. 4, P1.8e |
| **Microframe** | L'unità di tempo del bus USB veloce: 125 µs | Cap. 2, La scheda audio |
| **Nodo** | Un elemento del grafo: ingresso, uscita, guadagno, somma, bus, ritardo, generatore, misuratore, carico | Cap. 2, Il grafo |
| **Nodo abbandonato** | Un nodo in cui un thread si è bloccato: silenzio da subito, poi tolto dal grafo | Cap. 2, Il grafo |
| **Passa-tutto** (_all-pass_) | Un filtro che cambia la fase e non il livello | Cap. 2, Il calcolo |
| **Patch** | Il «pezzo del suono»: gli strumenti di misura che lavorano sui blocchi della scheda | Cap. 2, Il motore |
| **Percentile** | «Il 99,9-esimo percentile è X»: 999 misure su mille stanno sotto X | Cap. 3 |
| **Percorso critico** | La catena più lunga di nodi che si aspettano: il ciclo non può durare meno | Cap. 2, Il grafo |
| **Periodo** | Il blocco di campioni che la scheda scambia in un colpo, e il tempo che dura: 2 ms | Cap. 2, L'audio come numeri |
| **ppm** | Parti per milione: l'unità in cui si dice di quanto due orologi differiscono | Cap. 4, P1.6a |
| **Predittore dei salti** | La parte del processore che indovina dove andrà un salto prima di saperlo. Il kernel lo fa ripulire (IBPB) per difesa dagli attacchi di tipo Spectre | Cap. 2, Il tempo reale |
| **Programma che chiama** (un core live) | Il programma che, da un altro processore, chiede a un core live di eseguire una funzione del kernel. Lo dice `tools/live-callers.sh` | Cap. 3; cap. 4, P1.9a |
| **PREEMPT_RT** | La variante real-time del kernel Linux | Cap. 2, Il tempo reale |
| **Profilo di avvio** | Le opzioni del kernel che dipendono dalla macchina: `285k`, `generic` | Cap. 2, La piattaforma |
| **Quanto** | La lunghezza di un periodo in frame: 96, 192 o 384 secondo la frequenza | Cap. 2, L'audio come numeri |
| **Quota** (_share_) | La parte della potenza di un segnale ascoltato che è il tono di prova: 1,0000 se è pulito | Cap. 3, strumenti nel motore |
| **Raffica** (_burst_) | Un breve rumore suonato e riconosciuto al ritorno: misura il giro attraverso i convertitori | Cap. 3, strumenti nel motore |
| **RAPL** | I limiti di potenza del processore, e i contatori da cui si legge quanto consuma | Cap. 2, Il tempo reale |
| **Rapporto d'avvio** | La riga `DAWOS-BOOT-REPORT` che il sistema scrive a ogni avvio | Cap. 3 |
| **Recupero** | Far ripartire i flussi della scheda dopo un xrun | Cap. 2, L'audio come numeri |
| **Registro dei cicli** | Una riga per ogni ciclo di ogni thread, da cui si ricavano tutte le statistiche | Cap. 2, Il motore |
| **Retroazione implicita** | L'uscita della scheda regolata sul ritmo dell'ingresso | Cap. 2, La scheda audio |
| **Riferimento contato** (`Arc`) | Un oggetto che vive finché qualcuno lo tiene: così un thread tiene il grafo del suo ciclo | Cap. 2, Il grafo |
| **Riscaldamento** (_warm-up_) | I primi secondi di un giro, contati a parte e non giudicati | Cap. 2, La scheda audio |
| **Riserva** (_spare_) | Il thread tenuto pronto per prendere il posto di uno bloccato | Cap. 2, Il motore |
| **Ritardo di risveglio** | Il tempo tra quando un thread doveva ripartire e quando è ripartito | Cap. 2, Il tempo reale |
| **Ritocco** (_trim_) | Una moltiplicazione semplice su tutto il blocco: serve a comporre il costo esatto di un carico | Cap. 2, Il calcolo |
| **Routing** | Quale segnale va dove, dentro l'interfaccia | Cap. 2, La scheda audio |
| **Salto** (del tempo presso il controller) | In `usb-look.sh`: un trasferimento d'uscita che resta presso il controller 60 µs o più in più o in meno del precedente. Un salto che resta (da 1,115 a 1,24 ms) è un microframe saltato | Cap. 3; cap. 4, P1.8e |
| **Scambio** (_swap_) | Sostituire il grafo mentre il ciclo gira | Cap. 2, Il grafo |
| **SCHED_FIFO** | La classe di schedulazione real-time: gira chi ha la priorità più alta, finché non cede | Cap. 2, Il tempo reale |
| **Scrittura corta** (_short write_) | Un periodo di uscita che la scheda non prende per intero perché il suo buffer è pieno: ciò che non entra non viene suonato | Cap. 3, Le righe della scheda |
| **Secure Boot** | Il firmware avvia solo file firmati | Cap. 2, La piattaforma |
| **Segmento** | Una catena di nodi che va per intero su un thread | Cap. 2, Il grafo |
| **Segno** (_stamp_) | Il numero dell'ultimo ciclo per cui un nodo è stato eseguito: lo legge chi lo aspetta | Cap. 2, Il grafo |
| **Slittamento** (_slip_) | Il segnale che si sposta di qualche campione senza che nessuno segnali un errore | Cap. 2, L'audio come numeri |
| **Slot** | Il posto di un thread live nel motore (lo slot 0 tiene la scheda); anche, uno dei due posti del sistema sul disco | Cap. 2 |
| **SMI** | Un'interruzione del firmware che ferma tutti i processori all'insaputa del sistema | Cap. 2, Il tempo reale |
| **snd-aloop** | La scheda di software usata nella macchina virtuale | Cap. 2, La scheda audio |
| **Stadio** (_stage_) | Una sezione di filtro passata su tutto il blocco: l'unità di costo del carico | Cap. 2, Il calcolo |
| **Syscall User Dispatch** | Il meccanismo con cui il motore ferma le proprie chiamate di sistema nel calcolo | Cap. 2, Il tempo reale |
| **Thread** | Un filo di esecuzione di un programma. I thread live sono quelli del ciclo audio | Cap. 2, Il motore |
| **Throttling** | Il processore che rallenta da solo perché è troppo caldo | Cap. 2, Il tempo reale |
| **Tick** | L'interruzione periodica del timer del kernel; sui processori live è fermata | Cap. 2, Il tempo reale |
| **TLB** | La cache in cui il processore tiene le traduzioni degli indirizzi di memoria. Quando un processo restituisce o sposta memoria, il kernel la fa svuotare ai processori su cui gira (A-75) | Cap. 2, Il tempo reale |
| **Tono** | Una sinusoide di prova con un numero intero di cicli nel quanto | Cap. 3, strumenti nel motore |
| **Traccia** (_trace_) | La registrazione che il kernel fa dei propri eventi, quando gli si chiede | Cap. 3, Il registro dei cicli |
| **Trappola** (`trap`) | Un nodo di prova che dopo un certo tempo non ritorna più, tenendo ciò su cui lavorava | Cap. 3, La prova audio |
| **UKI** | Kernel e riga di comando in un solo file firmato | Cap. 2, La piattaforma |
| **`unsafe`** | Codice Rust che il compilatore non può verificare: ammesso in un solo modulo | Cap. 2, Il calcolo |
| **URB** | Il blocco di lavoro del driver USB (un trasferimento): per l'audio, da uno a otto pacchetti, cioè da 125 µs a un millisecondo secondo il periodo chiesto | Cap. 2, La scheda audio |
| **usbmon** | Lo strumento del kernel che mostra ogni trasferimento che passa su un bus USB | Cap. 3 |
| **Verdetto** | La riga `PASS` o `FAIL` con cui uno strumento giudica sé stesso | Cap. 3, I verdetti |
| **Xrun** | La scheda rimasta senza campioni da suonare, o senza posto per quelli registrati | Cap. 2, L'audio come numeri |

## Le sigle del progetto

Ogni sigla è una lettera, che dice di che cosa si tratta, e un numero d'ordine.

| Sigla | Che cos'è | Dove sta |
| --- | --- | --- |
| **R-nn** | Un requisito: ciò che il sistema deve fare | Il progetto iniziale |
| **K-nn** | Un rischio tecnico: ciò che può andare male | `docs/00-revisione-specifiche-fase0.md`, piano della Fase 1 |
| **C-nn** | Una contraddizione trovata tra due decisioni del progetto | `docs/00-revisione-specifiche-fase0.md` |
| **M-nn** | Una parte che nel progetto mancava | `docs/00-revisione-specifiche-fase0.md` |
| **D-nn** | Una domanda aperta, che diventa una decisione | Le guide dei passi, poi gli ADR |
| **E-nn** | Un esperimento della Fase 0 | Le guide P0.6 |
| **A-nn** | Un punto aperto: una cosa vista e non spiegata, o rinviata | `docs/02-punti-aperti.md` |
| **Q-nn** | Una questione aperta del documento di progetto | Entrano come A-nn |
| **P0.n, P1.n** | Un passo della Fase 0 o della Fase 1 | `docs/fasi/` |
| **ADR 00nn** | Una decisione di architettura, con le alternative scartate (_Architecture Decision Record_) | `docs/adr/` |

Un'avvertenza: in `docs/01-registro-modifiche-v1.md` la lettera R numera anche le **modifiche** al progetto iniziale. Fuori da quel file, R-nn è sempre un requisito.

## Le sigle che ricorrono

| Sigla | Che cosa dice | Stato |
| --- | --- | --- |
| **R-01** | La latenza del giro completo sotto 10 ms | Rispettato sulla seconda generazione; sulla quarta non a 44,1 e 48 kHz (A-69) |
| **R-04** | La sessione di riferimento (256 tracce, otto plugin in media, 96 kHz) deve stare entro il 60 % del dominio live e il 75 % dell'anticipativo | Il carico è definito e calibrato (P1.7d); la sessione è della Fase 2 |
| **K-01** | Il firmware ruba tempo ai processori (SMI) | Misurato: zero |
| **K-03** | R-01 ha poco margine | Vero sulla quarta generazione |
| **K-04** | Un thread real-time che non cede affama il kernel sul suo processore | Protezione fatta e provata (P1.3), anche per i nodi del grafo (P1.7f) |
| **K-07** | L'interruzione USB condivisa con altri dispositivi | Aperto: la Scarlett condivide il controller (A-53) |
| **K-08** | Difetti audio che ALSA non segnala come xrun | Visti: gli slittamenti. Per questo esiste il conteggio |
| **K-17** | Il risveglio del motore attraversa due processori | Misurato in P1.4 e P1.6 |
| **K-22** | Misurare disturba | Vero: accendere una traccia interrompe tutti i processori |
| **K-23** | L'attesa attiva tiene svegli i processori live | Misurato in P1.7c: attese sotto il microsecondo |
| **D-10** | Quale thread esegue un nodo: assegnazione statica | Deciso, ADR 0025 |
| **D-11** | Come è formulata la regola sulle chiamate di sistema, fase per fase | Deciso in P1.1 |
| **D-14** | La forma del buffer: `96:3` | Deciso in P1.5 |
| **D-26** | Chi sveglia i thread live: tutti aspettano la scheda | Deciso in P1.1; il modo trovato in P1.6b |
| **E-01** | Quanto costa svegliarsi da ogni stato di riposo | C1 (P0.6b) |
| **E-02** | Quanto costa spegnere il fair server | Spento (P0.6b) |
| **E-03, E-04** | Le regole dei gruppi di controllo; il driver grafico | P0.6c |
| **E-05** | Lo streaming da cartelle cifrate | P0.6d |
| **E-06** | Quanto vale un E-core rispetto a un P-core | 0,51–0,59 (P0.6d) |
| **A-49** | Le chiamate tra processori che restano: una serie di patch del kernel da provare (Q-13) | Aperto, P1.9 |
| **A-50** | Se serve un driver audio nostro (Q-14) | Aperto, P1.8 |
| **A-55** | Il risveglio a 7,5 µs con quattro thread | Spiegato e accettato (P1.6c) |
| **A-57, A-58, A-62** | L'assestamento, gli slittamenti, i flussi che partono corti: sulla seconda generazione | Aperti; sulla quarta non visti |
| **A-59, A-60** | Il giro che cambia da un avvio all'altro; il pavimento a 5,4 ms | Aperti, P1.8 |
| **A-61** | I periodi «saltati» senza xrun | Chiuso: è la scheda (P1.6a) |
| **A-68** | Il periodo annunciato a scatti di un millisecondo | Il motore ne tiene conto (P1.7b); la causa nel driver è aperta |
| **A-69** | La latenza più lunga sulla quarta generazione | Aperto, P1.8 |
| **A-70** | Una volta, in macchina virtuale, i thread non sono partiti in tempo | Aperto: causa ignota |
| **A-71** | Lo stadio costa il 14 % più che in C, e dipende dalla lunghezza del blocco | Aperto |
| **A-72** | Una scrittura corta all'avvio dei flussi, circa un avvio su tredici a 48 kHz | Aperto: si sa quando (il primo periodo), non perché. P1.8 |
| **A-73** | Il compilatore del grafo sottostima il ciclo su più thread, del 6–7 % su quattro | Aperto, Fase 2 |
| **A-74** | Chi aspetta un nodo di un thread bloccato taglia i suoi cicli per intero | Aperto, Fase 2 |
| **A-75** | Il processo fa svuotare la TLB sui propri processori live: 12 volte in un'ora di scambi, lo stesso numero in due ore | Aperto, P1.9 |
| **A-76** | Ogni programma BPF classico caricato (una connessione SSH, per esempio) fa ripulire il predittore dei salti su tutti i processori, live compresi | Aperto, P1.9 |
| **A-77** | Un errore di `fakeroot` nella costruzione dell'immagine, visto due volte: non si sa che cosa lasci a metà | Aperto |
