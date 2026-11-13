# ADR 0011 — R-04 con carico sintetico di riferimento

- **Stato**: accettata; un punto da confermare (in fondo)
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

1. l'**utilizzo di ogni dominio non supera il 60 %**, dove l'utilizzo è il tempo occupato dei core del dominio diviso per il tempo disponibile (numero di core × durata). Per il dominio live si prende il massimo sui singoli cicli; per il dominio anticipativo il massimo su finestre di un secondo;
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

## Conseguenze

- K-02 scende da «non misurabile» a «misurabile e al limite sul dominio anticipativo».
- Il rapporto E-core / P-core diventa il numero più importante da misurare presto. Propongo di anticiparlo al passo P0.6 con un banco di prova minimo (stesso nucleo DSP del carico sintetico, eseguito su un P-core e su un E-core alle frequenze fissate): costa poche ore e dice subito se R-04 è realistico.
- Se la misura conferma il superamento, le leve sono, in ordine di preferenza: ottimizzare il percorso anticipativo (blocchi più lunghi, meno salti); spostare parte del lavoro anticipativo su core oggi assegnati ad altro; ridurre la sessione di riferimento.
- La verifica del budget prima di attivare un modulo o inserire un plugin usa lo stesso modello di costo e lo stesso 60 %.

## Punto da confermare

**Il freeze è ammesso nella sessione di riferimento?** La mia raccomandazione:

- a **96 kHz** la sessione di riferimento si misura **senza** tracce congelate: è il caso che deve reggere da solo, e se non regge vogliamo saperlo;
- la **variante stress a 192 kHz** si misura **con il freeze ammesso**, dichiarando quante tracce è stato necessario congelare; il requisito diventa «le 16 tracce live e almeno N tracce non congelate», con N da fissare dopo la prima misura.

## Alternative scartate

- **Plugin commerciali reali come riferimento**: non ripetibile, dipende da prodotti e versioni di terzi.
- **Un solo costo medio**: nasconde il caso peggiore sul singolo core, che è quello che fa mancare una scadenza.

## Quando si riapre

Alla prima misura del rapporto E-core / P-core, e alla chiusura della Fase 4 con la forma VST3 del carico.
