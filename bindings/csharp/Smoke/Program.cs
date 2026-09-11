// Lightweight smoke for the RustPdf .NET binding. Verifies the native libpdf_ffi
// loads and basic operations work. Used by the release CI per platform. Exits
// non-zero on any failure.

using RustPdf;

static void Assert(bool cond, string msg)
{
    if (!cond)
        throw new Exception("SMOKE FAILED: " + msg);
}

Console.WriteLine($"rustpdf version: {Pdf.Version()}");
Assert(!string.IsNullOrWhiteSpace(Pdf.Version()), "version is empty");

// A plain PDF — every feature is free.
byte[] data;
using (var doc = new Document())
{
    doc.SetInfo(title: "smoke");
    doc.AddPage();
    doc.SetFillRgb(0.1, 0.2, 0.8).Rect(72, 72, 200, 100).Fill();
    data = doc.ToBytes();
}

Assert(data.Length > 0, "empty PDF output");
Assert(data.Length >= 5 && data[0] == (byte)'%' && data[1] == (byte)'P'
       && data[2] == (byte)'D' && data[3] == (byte)'F', "missing %PDF header");

Console.WriteLine($"OK — generated a {data.Length}-byte PDF");
