# ADR 0020 — Plugin Linux di terze parti

- **Stato**: accettata dall'utente il 18 novembre 2026. Nulla è ancora nell'immagine: il caricamento dei plugin arriva con il motore e con l'host dei plugin
- **Data**: 18 novembre 2026
- **Origine**: documento di progetto, **ADR-034** (nel repository la numerazione è un'altra: vedi la [tabella di corrispondenza](README.md)); nota dell'utente del 18 novembre 2026, punto 1

## Contesto

Una DAW senza plugin di terze parti serve a pochi. I plugin commerciali esistono soprattutto per Windows e macOS; una parte, in crescita, esiste anche come binario Linux nativo. dawOS è un sistema con la radice in sola lettura, verificata, e con un insieme di librerie che decide lui: un plugin compilato altrove funziona solo se trova le librerie che si aspetta, nelle versioni che si aspetta.

Due decisioni già prese preparano questa: la toolchain con glibc, scelta proprio perché i binari VST3 di terze parti la richiedono ([ADR 0006](0006-toolchain-bootlin-glibc.md)), e l'eccezione per i plugin nella licenza ([ADR 0010](0010-licenza-gplv3-eccezione-plugin.md)).

## Decisione

**dawOS carica plugin Linux nativi di terze parti. I plugin che esistono solo per Windows non sono supportati.**

### 1. Formati

| Formato | Quando | Interfaccia |
| --- | --- | --- |
| VST3 | MVP | Editor del plugin tramite XWayland |
| CLAP | Subito dopo VST3 | Editor tramite XWayland; pool di thread dell'host |
| LV2 | Dopo CLAP | Solo audio, con un pannello di parametri generato dall'host; niente interfacce GTK |

### 2. Runtime dei plugin

Un insieme di librerie versionato, parte dell'immagine: glibc, libstdc++, libgcc_s, X11 e xcb, Mesa (GL, EGL), freetype, fontconfig, libcurl, libasound, zlib.

- L'elenco definitivo si ricava da un corpus di plugin di riferimento: le librerie richieste (`readelf -d`) e le versioni dei simboli di glibc.
- **Togliere una libreria dal runtime è un cambiamento incompatibile.**

### 3. Cartella dei plugin

A scelta dell'utente:

- per utente, nella home: `~/.vst3`, `~/.clap`, `~/.lv2`;
- di sistema, sulla partizione `shared`, installabile solo con il ruolo di amministratore.

### 4. Installazione

1. Il file scaricato nella macchina virtuale ([ADR 0019](0019-apertura-del-sistema-utilita-native-e-vm-linux.md)) passa dalla cartella Scambio all'utilità «Installa plugin».
2. L'utilità verifica: ELF x86-64; librerie presenti nel runtime; versione di glibc; `moduleinfo.json` per i VST3; una prova con il validatore, in una sandbox.
3. Uno script di installazione del produttore gira in un _namespace_ confinato, dove `/usr/lib/vst3` e simili puntano alla cartella scelta.
4. Poi la scansione, isolata e con un tempo massimo.

### 5. Attivazione delle licenze

- Una finestra di rete temporanea, aperta dall'utente per un solo plugin; mai in Recording o in Live.
- **iLok non è supportato**: non esiste un client per Linux.

### 6. Thread creati dai plugin

- L'host li rileva (gruppo di controllo in modalità _threaded_, oppure `/proc/self/task`) e li riporta sui core di servizio.
- Con CLAP il plugin chiede i thread al pool dell'host.

### 7. Editor sugli schermi 4K

- La scala si comunica al plugin con `IPlugViewContentScaleSupport::setContentScaleFactor`; la finestra è a risoluzione nativa.
- Ripiego: ingrandimento per singolo plugin.

### 8. Accesso ai file

Le proprie cartelle di dati in scrittura, la cartella Libreria dell'utente in lettura, nient'altro.

## Alternative scartate

Il confronto completo sta nel documento di progetto. Qui ciò che la nota del 18 novembre 2026 esclude in modo esplicito.

| Alternativa | Perché no |
| --- | --- |
| Plugin solo Windows, con uno strato di compatibilità | Non supportati: fuori dal perimetro. |
| iLok | Non ha un client per Linux. |
| Interfacce GTK dei plugin LV2 | Escluse: l'host genera un pannello dai parametri. Il motivo, come lo ricostruisco io: porterebbero nel runtime una libreria grafica intera per un solo formato _(da confermare sul documento di progetto)_. |
| Rete sempre aperta ai plugin | Esclusa: la rete si apre solo per l'attivazione, per un plugin alla volta, e mai in Recording e in Live. |

## Conseguenze

- **Il runtime è un impegno di compatibilità**: ogni libreria che entra resta, alla versione di interfaccia con cui è entrata. Va versionato e provato a ogni aggiornamento del sistema contro il corpus di riferimento.
- **Serve XWayland** nel compositor di dawOS, per gli editor VST3 e CLAP.
- **La cartella di sistema dei plugin sta su `shared`**, che non è cifrata ([ADR 0004](0004-partizioni-dei-dati.md)): i plugin installati lì sono visibili a tutti gli utenti della macchina.
- **Il codice di terze parti gira dentro l'host dei plugin.** Le verifiche del punto 4 riducono il rischio all'installazione, non durante l'esecuzione.
- **Un thread che un plugin crea da sé** e che l'host sposta fuori dai core live non ha più le garanzie dei core live: un plugin che fa DSP su thread propri, fuori dal pool dell'host, avrà ritardi che il motore non controlla.

Punti che la decisione lascia da precisare, e che ho segnato nel [registro dei punti aperti](../02-punti-aperti.md) (A-46):

1. **«Core di servizio»** non è un dominio del profilo: lo leggo come dominio `system`, o come dominio anticipativo per i thread che fanno DSP. Da decidere, perché cambia il conto della capacità dell'[ADR 0011](0011-r04-carico-sintetico.md).
2. **Un thread real-time di un plugin che non cede mai il core** è il caso del punto aperto A-18: la protezione va progettata sapendo che i thread da sorvegliare non sono solo quelli del motore.
3. **Il corpus di riferimento** va scelto: quali plugin, di quali produttori, e chi lo mantiene.

## Quando si riapre

- Se dal corpus di riferimento risulta che il runtime deve contenere una libreria che non si vuole mantenere stabile.
- Se un formato cambia in modo incompatibile, o se uno dei tre perde il supporto dei produttori.
- Se esce un client iLok per Linux.
- Se si decide di supportare i plugin Windows.
