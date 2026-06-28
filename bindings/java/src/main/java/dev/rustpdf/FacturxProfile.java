package dev.rustpdf;

/** ZUGFeRD / Factur-X conformance profile for embedded e-invoice XML. */
public enum FacturxProfile {
    MINIMUM(0), BASIC_WL(1), BASIC(2), EN16931(3), EXTENDED(4);

    final int code;

    FacturxProfile(int code) {
        this.code = code;
    }
}
