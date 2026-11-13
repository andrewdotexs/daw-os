# ADR 0006 — Toolchain esterna Bootlin con glibc

- **Stato**: accettata
- **Data**: 13 novembre 2026
- **Origine**: D-06 del documento 0

## Contesto

I plugin VST3 di terze parti sono binari compilati contro glibc e libstdc++: musl e uClibc sono esclusi di fatto. Resta da scegliere tra una toolchain già pronta e una costruita da Buildroot.

## Decisione

Toolchain esterna Bootlin, variante **x86-64-v3, glibc, stable**, nella versione fissata da Buildroot 2026.02.3: `2025.08-1`, con GCC 14 e intestazioni del kernel 5.4 _(verificato sul tag)_.

Opzione: `BR2_TOOLCHAIN_EXTERNAL_BOOTLIN_X86_64_V3_GLIBC_STABLE`.

Il livello x86-64-v3 (AVX2, FMA, BMI2) vale per tutto lo spazio utente ed è coerente con la decisione sul motore.

## Alternative scartate

- **Toolchain costruita da Buildroot**: controllo completo su versioni e intestazioni, ma la prima costruzione sale di una o due ore.
- **Variante «bleeding edge» di Bootlin**: compilatore più recente, meno collaudato.

## Conseguenze

- Le **intestazioni del kernel della toolchain sono 5.4**, molto più vecchie del kernel 6.18 in esecuzione. Per il codice C dei pacchetti significa che le interfacce recenti (funzioni nuove di `io_uring`, Landlock, `futex2`) non sono dichiarate. Il codice Rust non ne risente, perché usa le definizioni dei propri crate. Va tenuto presente a ogni pacchetto C che richieda interfacce recenti.
- L'immagine non gira su CPU senza AVX2. In QEMU senza KVM serve `-cpu max`; lo script di avvio lo fa da sé.
- La versione di glibc dell'immagine fissa il limite superiore dei plugin caricabili: un plugin compilato contro una glibc più nuova non si carica. Va annotata nel runtime per plugin (M-09).

## Quando si riapre

Se un pacchetto C richiede intestazioni più recenti, o se nella Fase 4 la glibc della toolchain risulta troppo vecchia per plugin diffusi. In quel caso si passa alla toolchain costruita da Buildroot con intestazioni 6.18.
