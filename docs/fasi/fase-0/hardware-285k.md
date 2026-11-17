# Hardware di riferimento — stato delle conoscenze

- **Scopo**: raccogliere in un solo posto ciò che sappiamo del 285K prima del passo P0.4, distinguendo ciò che è dichiarato, ciò che è verificato e ciò che manca.
- **Aggiornato**: 13 novembre 2026.
- **Regola**: indirizzi di rete, numeri di serie e indirizzi fisici delle schede non entrano nel repository; stanno in `local/`.

## Cosa sappiamo

| Voce | Valore | Fonte |
| --- | --- | --- |
| Processore | Intel Core Ultra 9 285K | Schermata di Windows sul 285K |
| RAM | 64 GB, in funzione a 4800 MT/s | Schermata di Windows |
| Scheda madre | ASUS ROG STRIX Z890-I GAMING WIFI, Mini-ITX, chipset Z890 | Dichiarata; confermata dal kernel al primo avvio |
| Firmware | Versione **3402** del 18 settembre 2026, microcodice `0x122` (dal 16 novembre 2026; prima 2301 del 4 ottobre 2025, microcodice `0x11a`) | Letta dal kernel; aggiornamento nel sotto-passo P0.6c, parte 0 |
| Processori visti dal kernel | 24, un solo package, nessun thread aggiuntivo per core | Registro di avvio del 13 novembre 2026 |
| Schede grafiche | Intel Graphics integrata e NVIDIA GeForce RTX 5080 da 16 GB | Schermata di Windows |
| Archiviazione | 932 GB visibili, 123 GB usati | Schermata di Windows |
| Sistema attuale | Windows 11, nome `ANDREW-PC` | Schermata di Windows |
| Rete | Cablata, indirizzo IPv4 impostato a mano in Windows | Schermata di Windows |

Dalla scheda tecnica del produttore _(verificato sulla pagina ASUS, non sulla macchina)_:

| Componente | Dato | Conseguenza per dawOS |
| --- | --- | --- |
| Ethernet | Controller Intel da 2,5 Gb | Driver `igc` _(confermato al primo avvio: l'interfaccia `eno1` riceve la configurazione legata a quel driver)_; modello esatto da leggere con `lspci`. |
| Wi-Fi e Bluetooth | Modulo Wi-Fi 7 2x2 | Resta spento: nessun driver nel kernel. |
| Audio | Nessun codec sulla scheda: tutto passa dal modulo esterno ROG Strix Hive II, collegato via USB | Non lo usiamo; va tenuto scollegato o su un controller USB diverso da quello della Scarlett. |
| Thunderbolt | Due porte Thunderbolt 4 sul pannello posteriore | Sono un controller USB distinto da quello del chipset: è il candidato per isolare la Scarlett (rischio K-07). |
| M.2 | Due alloggiamenti PCIe 5.0; il secondo toglie corsie allo slot della scheda grafica | Spazio per un secondo NVMe, se servisse. |
| Slot | Un solo PCIe x16 | Occupato dalla RTX 5080. |

Non ho trovato nella scheda tecnica: la presenza di un connettore seriale e i dettagli del TPM. Su una Mini-ITX di questa serie un connettore seriale è improbabile _(da verificare sul manuale)_; il TPM sarà quello integrato nel processore (Intel PTT) _(da verificare)_.

## Cosa vede il kernel (13 novembre 2026)

Da `lspci -k`, `lsblk` e dal registro del primo avvio dal disco. Il numero di serie del disco non è riportato qui.

| Dispositivo | Indirizzo PCI | Driver in uso | Nota |
| --- | --- | --- | --- |
| Ethernet Intel I226-V (rev 06) | `83:00.0` | `igc` | Confermato. Interfaccia `eno1`. |
| Disco Samsung SSD 990 PRO 1TB | `02:00.0` | `nvme` | 931,5 GiB reali. Unico disco interno. |
| Intel VMD (controller RAID NVMe) | `00:0e.0` | `vmd` | VMD è attivo nel firmware, ma il disco non è collegato attraverso di lui: sta su `0000:02:00.0`, sotto la porta `00:06.4` (verificato il 15 novembre 2026). Il driver non serve al disco di questa macchina. |
| USB, controller del processore (Thunderbolt 4) | `00:0d.0` | `xhci_hcd` | Primo dei due controller USB. |
| USB, controller del chipset | `80:14.0` | `xhci_hcd` | Secondo controller USB. |
| Thunderbolt 4 NHI | `00:0d.2` | nessuno | Il sottosistema Thunderbolt non è nel kernel. |
| Grafica integrata Arrow Lake-S | `00:02.0` | nessuno all'avvio; `xe` a richiesta | La console usa il framebuffer del firmware. Dal sotto-passo P0.6c il driver `xe` è nell'immagine come modulo e si carica solo con `dawos-gpu-test --load` (identificativo `7d67`, da forzare: per Arrow Lake è sperimentale in Linux 6.18; `i915` non si compila su un kernel real-time). Provato il 16 novembre 2026: firmware DMC 2.23, GuC 70.49.4, HuC 8.5.4, GSC 102.1.15; la presa HDMI della scheda compare come `DP-2`; interrupt sui core 20–23. |
| NVIDIA RTX 5080 | `01:00.0` | nessuno | Come voluto. |
| Audio HDMI della RTX | `01:00.1` | `snd_hda_intel` | Nessun codec utile: errore all'avvio. |
| Intel HD Audio del chipset | `80:1f.3` | `snd_hda_intel` | Sulla scheda non c'è codec: tre secondi di tentativi ed errori all'avvio. |
| Intel Management Engine (HECI) | `80:16.0` | `mei_me` | Non serve. |
| SATA | `80:17.0` | `ahci` | Nessun disco collegato. |
| Wi-Fi 7 Intel BE200 | `84:00.0` | nessuno | Spento, come deciso. |
| NPU Arrow Lake | `00:0b.0` | nessuno | Servirà alla fase dell'agente. |

Conseguenze:

- **Due controller USB distinti** esistono davvero: la Scarlett potrà stare da sola su uno dei due (rischio K-07).
- **Tre driver da togliere dal kernel** nel passo P0.4b, perché inutili qui e sorgenti di lavoro all'avvio o di rumore: `snd_hda_intel` (l'audio di dawOS è USB), `mei_me`, e con ogni probabilità `ahci`.
- **ASPM**: il registro dice `igc: can't disable ASPM; OS doesn't have ASPM control`. Il firmware tiene per sé la gestione del risparmio energetico dei collegamenti PCIe. Per il tempo reale va spento dal firmware, voce già prevista nella lista del passo P0.4b.
- **Nessun avviso `W^X`** sull'hardware reale, come previsto nel passo P0.3: era un artefatto della macchina virtuale.

## Topologia dei processori (13 novembre 2026)

Dal rapporto archiviato in [`docs/misure/fase-0/topologia-285k.txt`](../../misure/fase-0/topologia-285k.txt), prodotto da `dawos-topology` sul 285K.

| CPU | Tipo | Cache L2 condivisa con | Frequenza massima | Nota |
| --- | --- | --- | --- | --- |
| 0 | P-core | nessuno | 5,6 GHz | CPU di avvio: non può essere `nohz_full`. |
| 1 | P-core | nessuno | 5,6 GHz | |
| 2 | P-core | nessuno | **5,8 GHz** | Uno dei due core «favoriti». |
| 3–6 | P-core | nessuno | 5,6 GHz | |
| 7 | P-core | nessuno | **5,8 GHz** | L'altro core «favorito». |
| 8–11 | E-core | 8–11 | 4,6 GHz | Primo cluster. |
| 12–15 | E-core | 12–15 | 4,6 GHz | Secondo cluster. |
| 16–19 | E-core | 16–19 | 4,6 GHz | Terzo cluster. |
| 20–23 | E-core | 20–23 | 4,6 GHz | Quarto cluster. |

Conferme e scoperte:

- **La numerazione è quella ipotizzata nel documento 0**: P-core sulle CPU 0–7, E-core sulle 8–23, quattro cluster da quattro E-core con la cache L2 in comune. Nessun thread aggiuntivo per core.
- **Disposizione fisica sul chip**, ricavata dagli identificativi dei core: `P P | E1 E2 | P P P P | E3 E4 | P P`. Le CPU 0–1 stanno a un'estremità, le CPU 2–5 al centro tra i due gruppi di E-core, le CPU 6–7 all'altra estremità. Conta per il calore: core vicini si scaldano a vicenda.
- **Due candidati per il dominio live**, da confrontare nel passo P0.6: le CPU 4–7 (ipotesi del documento 0; comprendono un core favorito ma sono due coppie separate) oppure le CPU 2–5 (quattro core contigui al centro, con l'altro core favorito). Confrontati nel sotto-passo P0.6a come due profili di avvio: stesso rumore, ma i core 4–7 sono risultati più freschi; l'utente ha scelto 4–7 il 16 novembre 2026.
- **Nessun driver per gli stati di riposo** (`cpuidle current_driver: none`): al kernel manca `CONFIG_INTEL_IDLE`. Oggi i core a riposo usano solo l'istruzione di arresto, senza stati profondi. Per l'esperimento E-01 (polling, C1, C1E) il driver va aggiunto, e nel firmware gli stati di riposo vanno lasciati al sistema operativo. **Corretto il 16 novembre 2026**: il driver è stato aggiunto e non cambia nulla, perché è il firmware a non dichiarare gli stati di riposo; vedi «Stati di riposo» nelle osservazioni.
- **Frequenza**: `intel_pstate` in modalità attiva, regolatore `powersave`, preferenza `balance_performance`, turbo acceso. È la configurazione di partenza; quella a frequenza fissa sui core live arriva con il passo P0.6.
- **Orologio**: `tsc`, come deve essere.
- **Rimappaggio degli interrupt attivo**: tutte le linee portano il prefisso `IR-`.
- **Esiste una porta seriale** (`ttyS0`, interrupt 4, con traffico): la console di sistema ci sta scrivendo. Una console seriale vera è lenta: nel profilo di avvio del 285K converrà toglierla dalla riga di comando, e sapere se sulla scheda c'è il connettore per usarla in diagnosi.

## Bus USB (13 novembre 2026)

| Bus | Controller | Cosa c'è collegato |
| --- | --- | --- |
| 1 e 2 | Processore (Thunderbolt 4), `00:0d.0` | **Nulla.** |
| 3 e 4 | Chipset, `80:14.0` | Tastiera e mouse, il controller dei LED, il modulo audio ROG Strix Hive II (senza driver), il Bluetooth (senza driver). |

Il controller del processore è libero: è il posto naturale per la Scarlett, da sola (rischio K-07). Resta **da verificare nella Fase 1** a quale controller arrivino davvero i segnali USB 2.0 delle porte Thunderbolt: su molte schede le linee USB 2.0 di quelle porte vanno al chipset. Si vede collegando la Scarlett e guardando `lsusb -t`.

## Osservazioni

**Il firmware consegna la macchina con x2APIC bloccato.** Il kernel deve quindi avere il rimappaggio degli interrupt (`CONFIG_IRQ_REMAP`), e nel firmware deve restare attivo VT-d. Senza, il kernel va in panico a 0,16 secondi dall'avvio. Scoperto al primo avvio reale; i dettagli sono nella guida del passo P0.4a.

**Uscite video.** Con il monitor sulla RTX 5080 l'avvio dalla chiavetta non mostrava nulla; con il monitor sull'uscita HDMI della scheda madre i messaggi del kernel sono visibili. Per dawOS il monitor va tenuto sulla scheda madre, coerentemente con la decisione di usare la grafica integrata.

**La RAM lavora a 4800 MT/s**, cioè al profilo di base e non a un profilo accelerato. Per il tempo reale è la scelta giusta: la lista del firmware nel passo P0.4 chiede proprio un profilo di memoria stabile. Non va cambiato.

**Mini-ITX con 285K e RTX 5080.** Lo spazio ridotto rende calore e rumore delle ventole più critici del previsto: pesa sul rischio K-04 e rende più importanti le misure dell'esperimento E-01 e la verifica di cosa fa la RTX senza driver (K-14).

**Raffreddamento: 85 °C già con 115 W** (curva termica del passo P0.4b, 13 novembre 2026, contenitore chiuso, firmware com'era). Temperatura del package a carico continuo: 66 °C a 60 W, 75 °C a 80 W, 81 °C a 100 W, 88 °C a 125 W, 97 °C a 150 W. Il contatto tra processore e dissipatore è nella norma (circa 0,27 °C per watt) e il radiatore lavora (circa 0,09 °C per watt); il problema è il punto di partenza: **il liquido è a 44 °C già a riposo**, con il processore che dissipa meno di 4 W. Cause da distinguere: aria calda nel contenitore, oppure ventole e pompa tenute al minimo dal firmware. Rischio K-15.

**Dati termici e di potenza letti dalla macchina** (13 novembre 2026): limite di giunzione 105 °C, ma la protezione termica interviene attorno a 97–100 °C; limiti di potenza impostati dal firmware PL1 = PL2 = 250 W con finestra di 56 secondi, **modificabili dal sistema operativo** (il firmware non li blocca); package a riposo 3,7 W e 45 °C, senza driver per gli stati di riposo profondi. Il dissipatore è un **be quiet! Pure Loop 2 FX da 280 mm** (due ventole da 140 mm, pompa PWM, distributore per ventole e luci). Il 13 novembre 2026 aveva **una sola ventola montata su due**: la seconda va montata. Restano da annotare i collegamenti di pompa e ventole e il verso dell'aria.

**Dopo l'intervento del 14 novembre 2026** (RTX 5080 tolta, seconda ventola montata): package a riposo 32 °C e 50 W alla presa; a carico continuo 53 °C a 60 W, 65 °C a 100 W, 79 °C a 150 W, 87 °C a 175 W, senza interventi della protezione termica. I valori del 13 novembre qui sopra descrivono la macchina com'era prima.

**Firmware, stato al 14 novembre 2026**: limite di potenza PL1 = PL2 = **160 W** (prima 250 W); ASPM, Wi-Fi, Bluetooth e _HD Audio_ disattivati; profilo di memoria di base (X.M.P. disattivato, 4800 MT/s); pompa a 5378 giri al minuto; ventola del processore sul profilo _Standard_, al 100 % da 70 °C. Voci cambiate rispetto a prima: Wi-Fi, Bluetooth e _HD Audio_ da attivi a disattivati; i due limiti di potenza da automatici a 160; l'ASPM era già disattivato. Verifica a carico continuo al limite: 83,3 °C con la stanza a 24 °C.

**Ventole del radiatore, 16 novembre 2026**: una delle due non era collegata al connettore della ventola del processore e partiva in ritardo sotto carico (circa quattro minuti). Corretto dall'utente. La curva termica e la verifica del limite a 160 W del passo P0.4b erano state fatte con quel collegamento. Ripetute lo stesso giorno con entrambe le ventole, stanza a 24 °C: circa 32 °C + 0,30 °C per watt (prima 35 °C + 0,30 °C per watt); a 160 W per dieci minuti 79,6 °C (prima 83,3 °C). Il limite resta 160 W. Dettagli nella guida del sotto-passo P0.6b.

**Secure Boot, stato al 15 novembre 2026**: attivo (_OS Type_: _Windows UEFI mode_; _Secure Boot Mode_: _Custom_; stato _User_). Fino a quel giorno il firmware era senza chiavi, in stato _Setup_. Ora contiene le chiavi di fabbrica (_PK_, _KEK_, _DBX_) e in _DB_ tredici firme: quelle di fabbrica più il certificato di sviluppo di dawOS. Procedura e verifica nella guida del sotto-passo P0.5f.

**Stati di riposo, stato al 16 novembre 2026** (sotto-passo P0.6b, parte 4a): con la voce _CPU C-states_ su _Auto_ (gli altri valori possibili sono solo _Disabled_) il firmware non dichiara al sistema alcuno stato di riposo: nessuna tabella nomina `_CST`, e il driver resta `none` anche con `CONFIG_INTEL_IDLE`. Il processore è bloccato al limite `pc0` (`MSR_PKG_CST_CONFIG_CONTROL` = `0x00008000`). Con _Enhanced C-states_ su _Auto_ il C1E automatico è acceso (`MSR_IA32_POWER_CTL` = `0x00f8005f`): ogni core fermo scende a C1E, e non più giù. Stati profondi (C6 e oltre) non ce ne sono, per nessun core _(se un'altra voce del firmware li renda disponibili non è stato cercato)_. **Dal 16 novembre 2026 _Enhanced C-states_ è su _Disabled_** (ADR 0009): i core fermi restano in C1, `MSR_IA32_POWER_CTL` = `0x00f8005d`. È un'impostazione richiesta dal profilo `285k`: se torna al valore di fabbrica, `dawos-tune` fallisce all'avvio.

**Firmware 3402, stato al 16 novembre 2026** (sotto-passo P0.6c, parte 0): aggiornato dall'utente con _EZ Flash_, insieme all'Intel ME. Le chiavi di Secure Boot sono sopravvissute. Voci rimesse dopo l'aggiornamento: limiti di potenza a 160 W, _Enhanced C-states_ su _Disabled_, _HD Audio_, _Wi-Fi Controller_ e _Bluetooth Controller_ su _Disabled_; ventola del processore sul profilo _Standard_, pompa a piena velocità. _Intel Default Settings_ va tenuto su _Performance_ (era il valore con la 2301; _Extreme_ alza il limite di picco da 425 a 490). Rispetto alla 2301: frequenza massima dichiarata 5,5 GHz sui P-core (prima 5,6) e 4,7 GHz sugli E-core (prima 4,6); scala delle prestazioni HWP a 88 livelli da 62,5 MHz sui P-core (prima 64 da 87,5 MHz). I due stati sono in `docs/misure/fase-0/firmware-2301.txt` e `firmware-3402.txt`. Nessun SMI in quindici minuti. Rimisurato lo stesso giorno: i core live tengono esattamente 3,7, 4,0, 4,3, 4,6 e 4,9 GHz a pieno carico; a 160 W per dieci minuti 79,8 °C; a riposo il package assorbe 3,0 W (prima 4,2).

**Processore e disco sotto carico, stato al 17 novembre 2026** (sotto-passo P0.6d):

- Su codice DSP vettoriale un E-core vale tra 0,51 e 0,59 di un core live a 4,6 GHz; un P-core rende in proporzione alla frequenza.
- Le frequenze vere sono lo 0,4 % sotto le nominali (4583 MHz dove il kernel dice 4600): il clock di base è 99,63 MHz.
- **Un P-core a riposo si risveglia tardi (fino a 55 µs) se altri P-core stanno lavorando a frequenza bassa.** Non succede se quelli lavorano a frequenza alta, né con gli E-core. Per questo i P-core che non sono live hanno la frequenza fissa al massimo; il package a riposo passa da 3 a 6 W.
- Il disco (Samsung 990 PRO da 1 TB) legge a 7,1 GB/s in sequenza e impiega circa 4,8 ms a svuotare la propria cache a ogni `fdatasync`.

**Stato al 17 novembre 2026, sera** (sotto-passo P0.6e, parte 2): kernel 6.18.64. TPM 2.0 del firmware visibile al sistema. Avvio in 20 secondi, di cui 14 del firmware e 0,4 del menu. Registri degli errori hardware: 32 per core, 11 senza interrupt proprio. Collegamenti PCIe: risparmio energetico spento su disco e scheda di rete; acceso su due porte per periferiche esterne, vuote. Il riavvio nelle impostazioni del firmware da sistema non è supportato: ci si entra con Canc all'accensione.

**Configurazione ferma per la campagna P0.7, dal 17 novembre 2026** (chiusura del sotto-passo P0.6e): immagine `0.0.1.20261117231225` dal commit `4cdf08c`, kernel 6.18.64, firmware 3402 con le voci di questa pagina, profilo `285k`, nessun modulo caricato. A riposo il package consuma 7–8 W.

**Contenitore: resta quello attuale (D-22, chiusa dall'utente il 15 novembre 2026).** Sono state valutate e scartate due alternative da rack: un contenitore metallico 2U da modificare, con il radiatore sopra il coperchio (14 novembre), e un telaio aperto 3U stampato in ASA, con il radiatore sul retro del rack (15 novembre). Il secondo avrebbe tolto ogni schermatura a pochi centimetri dalla scheda audio.

Conseguenza: la curva termica, il limite di potenza a 160 W e le misure di rumore di fondo dei passi P0.4b e P0.5f **restano valide** per i passi P0.6 e P0.7, e non vanno rifatte. Se in futuro il contenitore o il dissipatore cambiano, vanno rifatte prima di ogni misura di riferimento.

**Ambiente d'uso**: stanza climatizzata, al massimo 24–25 °C tutto l'anno (indicato dall'utente il 13 novembre 2026). È la temperatura di riferimento per il limite di potenza.

**Potenza alla presa** (Tapo P115, 13 novembre 2026): 2 W da spento, **77 W a sistema fermo**, e sotto carico 72 W + 1,16 × potenza del package (141 W a 60 W, 245 W a 150 W). Dei 77 W a riposo il processore ne spiega meno di 5: il resto è scheda madre, memoria, disco, pompa, ventole e RTX 5080 senza driver, quest'ultima plausibilmente per 30–45 W _(stima)_. È il primo numero per il rischio K-14. Deciso il 13 novembre 2026 di **togliere la RTX 5080 dal contenitore**: dawOS non la usa.

**Il modulo audio ROG Strix Hive II è un dispositivo USB.** Se resta collegato, il kernel lo vedrà come una seconda scheda audio USB. Nel passo P0.4 va deciso se scollegarlo; nel frattempo conta come dispositivo da tenere lontano dal controller della Scarlett.

## Il disco: deciso

**Decisione del 13 novembre 2026: caso A.** Il disco da 932 GB oggi occupato da Windows 11 è il disco dedicato a dawOS. Il passo P0.4 lo **cancella per intero**, Windows compreso. Il 285K non ha altri dischi da preservare.

Conseguenze:

- **Prima del passo P0.4** va salvato altrove tutto ciò che di quel Windows vuoi tenere (123 GB occupati oggi). Dopo l'installazione non è recuperabile.
- I dati della sezione seguente vanno raccolti **finché Windows è ancora lì**: dopo si possono leggere anche da dawOS, ma avere i due elenchi permette di confrontarli.
- Lo script di scrittura si aspetta **un solo disco NVMe interno**. Se al momento dell'installazione ne trova più d'uno, si ferma: vuol dire che la macchina non è più quella descritta qui.
- Le altre protezioni restano tutte: identificazione per modello e numero di serie, rifiuto del disco da cui si è avviato e dei dischi rimovibili, elenco di ciò che verrà cancellato, conferma digitata con il numero di serie, prova a secco come comportamento predefinito. Che il disco sia «solo» quello di Windows non le rende superflue: servono proprio a evitare di cancellare la chiavetta o un disco aggiunto nel frattempo.
- L'aggiornamento del firmware della scheda madre non richiede Windows: si fa dal firmware stesso, da una chiavetta. Non serve farlo prima di cancellare.

Le alternative scartate erano un secondo NVMe (caso B) e il mantenimento di Windows sul 285K (caso C), che avrebbe riaperto la decisione D-01.

## Dati da raccogliere finché c'è Windows

Sul 285K, in PowerShell aperto come amministratore. Ogni comando va copiato da solo; incollami le uscite, togliendo se vuoi i numeri di serie (quelli servono a te, in `local/`).

Dischi presenti, con modello e numero di serie:

```powershell
Get-PhysicalDisk | Format-List FriendlyName, SerialNumber, MediaType, BusType, Size
```

Scheda madre e versione del firmware:

```powershell
Get-CimInstance Win32_BaseBoard | Format-List Manufacturer, Product, Version
```

```powershell
Get-CimInstance Win32_BIOS | Format-List SMBIOSBIOSVersion, ReleaseDate
```

Stato del TPM e di Secure Boot:

```powershell
Get-Tpm | Format-List TpmPresent, TpmReady, ManufacturerIdTxt, ManufacturerVersion
```

```powershell
Confirm-SecureBootUEFI
```

Schede di rete:

```powershell
Get-NetAdapter | Format-Table Name, InterfaceDescription, LinkSpeed, Status
```

Server DNS configurati, che completano l'indirizzo impostato a mano:

```powershell
Get-DnsClientServerAddress -AddressFamily IPv4 | Format-Table InterfaceAlias, ServerAddresses
```

Controller USB:

```powershell
Get-PnpDevice -PresentOnly | Where-Object { $_.FriendlyName -match 'xHCI|Host Controller' } | Format-Table FriendlyName, Status
```

Un'avvertenza sul numero di serie: per i dischi NVMe Windows mostra a volte un identificativo diverso da quello che legge Linux. Il valore che fa fede per lo script di scrittura è quello letto da dawOS avviato dalla chiavetta; quello di Windows serve da confronto e per contare i dischi.

## Rete: deciso

**Decisione del 13 novembre 2026: indirizzo statico nell'immagine** (alternativa B).

- Indirizzo, maschera, gateway e server DNS stanno in un file di `local/`, ignorato da git, e vengono letti alla costruzione. Nel repository non compare alcun indirizzo.
- La configurazione statica si applica **solo alla scheda di rete del 285K**, riconosciuta dal suo driver; ogni altra scheda cablata, compresa quella virtuale di QEMU, continua a usare DHCP. Così la stessa immagine resta provabile in QEMU.
- Se il file in `local/` manca, l'immagine usa DHCP ovunque: è il comportamento di oggi.
- Il server DNS scelto è il router della rete locale, non i server pubblici oggi impostati in Windows.

Il collegamento risulta a 1000 Mb/s, su una scheda da 2,5 Gb: è il limite del resto della rete, e per SSH, aggiornamenti e carico di rete delle prove basta.

Si realizza nel passo P0.4, insieme al driver della scheda, che la configurazione predefinita del kernel potrebbe non includere _(da verificare)_.

Alternativa scartata: prenotazione sul router, che avrebbe lasciato l'immagine del tutto generica ma richiedeva di configurare il router.
