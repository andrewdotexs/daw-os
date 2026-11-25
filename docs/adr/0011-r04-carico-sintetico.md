# ADR 0011 — R-04 con carico sintetico di riferimento

- **Stato**: accettata; punto sul freeze confermato il 13 novembre 2026; rapporto tra E-core e P-core misurato il 17 novembre 2026 (esperimento E-06); lo stesso giorno la soglia del dominio anticipativo è stata portata al 75 %, e con quella la sessione di riferimento a 96 kHz passa
- **Data**: 13 novembre 2026
- **Origine**: revisione del documento 0, modifica 2; chiude D-16 e aggiorna K-02

## Contesto

R-04 chiede una sessione di riferimento di 256 tracce con in media 8 plugin ciascuna, 16 tracce live, a 96 kHz, più una variante stress a 192 kHz. Senza dire quanto costa un plugin, il requisito non è misurabile: passa con plugin leggeri e fallisce con plugin pesanti.

## Decisione

### Carico di riferimento

Il «plugin» di R-04 è un **carico sintetico calibrato**, in tre classi.

| Classe | Costo su P-core | Quota |
| --- | --- | --- |
| Leggera | 10 ns per campione | 60 % |
| Media | 40 ns per campione | 30 % |
| Pesante | 150 ns per campione | 10 % |

Precisazioni necessarie perché la misura sia ripetibile:

- **Campione** significa un _frame_: la coppia di campioni, sinistro e destro, di un'istanza stereo. Un'istanza leggera a 96 kHz costa quindi 0,96 ms di P-core per secondo di audio.
- **Su P-core** significa su un core live alla frequenza fissata dal profilo. Lo stesso carico su un E-core costa di più, come accadrebbe a un plugin vero: quel rapporto si misura, non si assume.
- Il carico è **lavoro DSP reale** (una cascata di filtri il cui numero di stadi è calibrato per raggiungere il costo dichiarato), non un'attesa a vuoto: deve stressare cache e unità vettoriali come un plugin.
- La calibrazione avviene una volta per versione dell'immagine, sul 285K, e il numero di stadi per classe si registra nel rapporto.
- L'assegnazione è deterministica: le istanze si numerano per traccia e per slot e ricevono la classe secondo il ciclo di dieci posizioni `L L L L L L M M M P`. Su 2048 istanze: 1229 leggere, 614 medie, 205 pesanti.
- Il carico esiste in due forme con lo stesso nucleo: nodo nativo (dalla Fase 1) e plugin VST3 (dalla Fase 4). R-04 completo si misura con la forma VST3.

Costo medio della miscela: 0,6 × 10 + 0,3 × 40 + 0,1 × 150 = **33 ns per campione**.

### Criterio di superamento

La sessione di riferimento passa se, per almeno 10 minuti di riproduzione con le 16 tracce live monitorate:

1. l'**utilizzo del dominio live non supera il 60 % e quello del dominio anticipativo non supera il 75 %**, dove l'utilizzo è il tempo occupato dei core del dominio diviso per il tempo disponibile (numero di core × durata). Per il dominio live si prende il massimo sui singoli cicli; per il dominio anticipativo il massimo su finestre di un secondo. La soglia del dominio anticipativo era il 60 % fino al 17 novembre 2026: vedi «Misura del 17 novembre 2026»;
2. nessuno dei quattro contatori di difetto (M-06) si muove;
3. nessun core live supera il 60 % nemmeno considerato da solo, contando la serializzazione dei segmenti esterni assegnati a quel core (documento 0, sezione 2.7 aggiornata).

### Freeze delle tracce

Il freeze entra nell'MVP: una traccia congelata viene resa su file e da quel momento costa solo lo streaming. È il meccanismo con cui l'utente riporta sotto soglia una sessione che eccede il budget.

## Cosa dicono i conti

Tutte stime, da sostituire con misure.

**Dominio live a 96 kHz.** 128 istanze × 33 ns × 256 campioni = 1,08 ms di lavoro per ciclo, su 4 core × 2,667 ms = 10,67 ms disponibili: circa il 10 %. Aggiungendo i salti tra processi (circa 24 × 10 µs) e i bus, circa il 15 %. A 192 kHz circa il 25–28 %. **Passa con largo margine.**

**Dominio anticipativo a 96 kHz.** 1920 istanze × 33 ns × 96 000 = 6,08 P-core equivalenti di lavoro continuo. I core sono 4 P-core e 8 E-core; l'esito dipende dal rapporto di prestazioni tra E-core e P-core su codice DSP vettoriale, che non conosco.

| Rapporto E-core / P-core | Capacità in P-core equivalenti | Utilizzo stimato |
| --- | --- | --- |
| 0,55 | 8,4 | 72 % |
| 0,65 | 9,2 | 66 % |
| 0,75 | 10,0 | 61 % |
| 0,80 | 10,4 | 58 % |

A questo si sommano mixaggio, misuratori e gestione dello streaming, qualche punto percentuale. **L'esito è al limite: con la stima centrale la soglia del 60 % non è rispettata.**

**Variante stress a 192 kHz.** Il lavoro raddoppia: tra il 117 % e il 145 % della capacità del dominio anticipativo. **Non può passare senza freeze.**

## Misura del 17 novembre 2026

Esperimento E-06, sul 285K con il firmware 3402 e il profilo `285k` (misura `dspbench-20261117T091510Z-e06-c`; guida in [P0.6d](../fasi/fase-0/P0.6d-rapporto-tra-core-e-streaming-cifrato.md)). Il nucleo del carico sintetico, una cascata di filtri del secondo ordine in AVX2, eseguito su un core live a 4,6 GHz e sui dodici core del dominio anticipativo.

Quanto vale un core, in core live:

| Core | Frequenza | Vale |
| --- | --- | --- |
| P-core del dominio anticipativo | 5,4 GHz (non è fissata) | da 1,13 a 1,17 |
| E-core | 4,6 GHz | da 0,51 a 0,59 |

- Un E-core vale **poco più di mezzo core live alla stessa frequenza**: il fondo della forbice stimata (0,55–0,80), e sotto con il codice vettoriale più denso.
- Un P-core rende in proporzione alla frequenza, e nient'altro lo distingue da un core live.
- I dodici core al lavoro insieme rendono quanto la somma dei dodici presi uno alla volta: non si disturbano, finché il package resta sotto il limite di potenza.

Capacità del dominio anticipativo e utilizzo della sessione di riferimento a 96 kHz (6,08 core live di lavoro). «Flussi» è il modo di caricare il core: con uno conta la durata di una singola operazione, con quattro conta quante operazioni vettoriali il core fa insieme; un plugin vero sta tra i due.

| Blocco | Flussi | Capacità, dominio al lavoro | Utilizzo | Package | Utilizzo con tutta la macchina a 160 W |
| --- | --- | --- | --- | --- | --- |
| 256 | 1 | 9,15 core live | 66 % | 79 W | 68 % |
| 256 | 4 | 8,71 | 70 % | 150 W | 77 % |
| 2048 | 1 | 9,44 | 64 % | 89 W | 66 % |
| 2048 | 4 | 8,83 | 69 % | 160 W | 77 % |

**Esito: la sessione di riferimento a 96 kHz usa tra il 64 % e il 70 % del dominio anticipativo. La soglia del 60 % non è rispettata**, per 4–10 punti; per rispettarla servirebbero 10,1 core live di capacità e ce ne sono tra 8,7 e 9,4. A questo si sommano ancora mixaggio, misuratori e gestione dello streaming. La variante a 192 kHz chiede tra il 129 % e il 140 %: come previsto, non passa senza freeze.

Due scelte dell'utente del 17 novembre 2026:

- **Fa testo la condizione con il dominio al lavoro da solo** (terza e quarta colonna). L'ultima colonna, con tutti i 24 core a pieno carico e il package al limite, si riporta come caso peggiore: descrive una macchina più carica di quella che R-04 chiede, dove i core live stanno al 15 %.
- **La leva: una soglia distinta per il dominio anticipativo, 75 %** (scelta alla chiusura del sotto-passo P0.6d, con i numeri di E-05). Con il dominio al lavoro da solo la sessione usa tra il 64 % e il 70 %: passa. Nel caso peggiore, con tutta la macchina a 160 W, tra il 66 % e l'81 %: non sempre. La soglia del dominio live resta il 60 %.

Perché questa e non le altre: il dominio anticipativo lavora in anticipo su un buffer e assorbe i picchi, mentre il dominio live ha una scadenza a ogni ciclo; la soglia unica era prudenza, non una misura. Non aggiunge capacità, e va detto: sposta il criterio. Ma non consuma core che altri si contendono. Nel frattempo è arrivata la macchina virtuale per le applicazioni (ADR 033, non ancora nel repository), che avrà bisogno di core e di cui non si conosce il fabbisogno.

Le leve che restano di riserva, per la Fase 2, quando si conosceranno il costo vero del motore e quel fabbisogno:

| Leva | Effetto | Prezzo |
| --- | --- | --- |
| Due E-core in più al dominio anticipativo (4 P-core e 10 E-core), presi a `system` o a `io` | Utilizzo tra il 57 % e il 62 % | Meno core per interfaccia, servizi o I/O; cambia il profilo `285k`. |
| Sessione di riferimento più piccola (7 plugin per traccia in media) | Utilizzo tra il 56 % e il 61 % | Cambia R-04. |

E-05 ha misurato che il dominio di I/O è largo: 2 % in chiaro, 11 % quando cifra tutto lui. Due dei suoi E-core sono la riserva più a portata.

La prima leva dell'elenco originale, ottimizzare il percorso anticipativo, non agisce su questo numero: qui è misurato il solo costo dei plugin, senza salti tra processi.

Altre due cose che la misura ha mostrato e che servono al motore:

- **Il codice vettoriale denso porta il package al limite con il solo dominio anticipativo**: con quattro flussi i dodici core chiedono 150–160 W, con uno 79–89 W. Il limite di potenza (160 W) è parte della capacità.
- **La posizione dei buffer in memoria conta**: con i buffer di più istanze allo stesso scostamento dentro la pagina il costo sale del 3,5 %, e in una esecuzione su dieci del 16 %. L'allocatore del motore deve sfalsarli.

Calibrazione indicativa delle classi sul core live: 24, 96 e 359 stadi per 10, 40 e 150 ns (0,4174 ns per stadio). Quella vera si fa con il nodo nativo, nella Fase 1.

## Il nodo nativo e la sua calibrazione (passo P1.7d, 25 novembre 2026)

Il carico in forma di nodo nativo esiste dal passo [P1.7d](../fasi/fase-1/P1.7-il-grafo.md). Tre precisazioni su ciò che questa decisione lasciava aperto.

- **Un'istanza stereo occupa un vettore intero**: è un banco di quattro bande in parallelo, sinistra e destra di ciascuna, otto valori. Uno «stadio» del nodo sono quindi quattro sezioni del secondo ordine, e fa lo stesso lavoro delle quattro istanze affiancate di `dawos-dspbench`. **Misurato il 25 novembre 2026**: 1,845 ns per frame sul core live, contro 1,622 di `dawos-dspbench` sullo stesso core (0,4056 per istanza): il 14 % in più, per una copia tra registri che il compilatore di Rust lascia nella catena tra un frame e il successivo. I 24, 96 e 359 stadi della calibrazione indicativa diventano 4, 20 e 80, con 2, 4 e 0 ritocchi.
- **Le sezioni sono passa-tutto**, con coefficienti diversi da banda a banda: stessa aritmetica del passa-basso di `dawos-dspbench`, ma il livello del segnale resta quello d'ingresso.
- **Il costo dichiarato si raggiunge con stadi e ritocchi.** Uno stadio è troppo grosso per colpire 10 ns da solo (passi di 1,67): dopo gli stadi che ci stanno, il resto si copre con «ritocchi», moltiplicazioni semplici da qualche decimo di nanosecondo per frame. Il costo dichiarato è tutto il costo dell'istanza a 96 kHz con 192 frame, compresa la parte fissa di ogni nodo del grafo.

**La calibrazione** sono tre cifre, in nanosecondi per frame su un core live: ciò che un'istanza costa prima di ogni filtro, ciò che aggiunge uno stadio, ciò che aggiunge un ritocco. Le misura `tools/graph-calibrate.sh` con il motore stesso; stanno in `Costs::default` del crate `dawos-graph`, e ogni rapporto del motore dice di quanti stadi e ritocchi è fatto ogni carico del grafo (righe `graph_load_of_…_ns`). Vanno rimisurate quando cambia il processore di riferimento, la frequenza fissata dal profilo, o il modo in cui il motore è compilato.

**Le cifre del 25 novembre 2026** (misura `calibrate-20261125T143805Z-p17d`): 2,3, 1,845 e 0,207 ns per frame. Con quelle le tre classi sono venute entro il 3 %, il 2 % e lo 0,2 % dal costo dichiarato.

**Il costo per frame dipende dalla lunghezza del blocco**: uno stadio costa 1,66 ns con 96 frame, 1,85 con 192, 1,95 con 384. Le classi costano quindi ciò che dichiarano a 96 kHz, dal 3 al 9 % in meno a 48 kHz e dal 6 al 7 % in più a 192 kHz. Per la variante stress a 192 kHz il carico va calibrato a quel quanto (punto aperto A-71).

## Conseguenze

- K-02 scende da «non misurabile» a «misurabile e al limite sul dominio anticipativo».
- Il rapporto E-core / P-core diventa il numero più importante da misurare presto. Si anticipa al passo P0.6 con un banco di prova minimo, l'esperimento E-06 **(approvato)**: stesso nucleo DSP del carico sintetico, eseguito su un P-core e su un E-core alle frequenze fissate. Costa poche ore e dice subito se R-04 è realistico.
- Se la misura conferma il superamento, le leve sono, in ordine di preferenza: ottimizzare il percorso anticipativo (blocchi più lunghi, meno salti); spostare parte del lavoro anticipativo su core oggi assegnati ad altro; ridurre la sessione di riferimento.
- La verifica del budget prima di attivare un modulo o inserire un plugin usa lo stesso modello di costo e lo stesso 60 %.

## Freeze nella sessione di riferimento

**Confermato** nella revisione del 13 novembre 2026:

- a **96 kHz** la sessione di riferimento si misura **senza** tracce congelate: è il caso che deve reggere da solo, e se non regge vogliamo saperlo;
- la **variante stress a 192 kHz** si misura **con il freeze ammesso**, dichiarando quante tracce è stato necessario congelare; il requisito diventa «le 16 tracce live e almeno N tracce non congelate», con N da fissare dopo la prima misura.

## Alternative scartate

- **Plugin commerciali reali come riferimento**: non ripetibile, dipende da prodotti e versioni di terzi.
- **Un solo costo medio**: nasconde il caso peggiore sul singolo core, che è quello che fa mancare una scadenza.

## Quando si riapre

Nella Fase 2, con il motore vero: se l'utilizzo misurato supera il 75 %, o se la macchina virtuale delle applicazioni toglie core al dominio anticipativo. E alla chiusura della Fase 4 con la forma VST3 del carico.
