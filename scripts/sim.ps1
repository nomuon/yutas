# Drives a real text field with synthetic key presses to test TypeBoost.
# Every key is sent only while the expected window is in the foreground.
#   -Keys   romaji to type with the IME on (letters, digits, '-', ',', '.')
#   -Title  substring the foreground window title must contain
#   -Undo   double-tap again afterwards to test undo
param(
    [string]$Keys = 'ashitanomi-thinguha15jikaradesu',
    [Parameter(Mandatory)][string]$Title,
    [switch]$Undo,
    [int]$WaitMs = 2500,
    # Key to double-tap: 0xA0 left Shift (default trigger), 0x1C 変換
    [int]$TriggerVk = 0xA0,
    # Clean up afterwards: 'notepad' empties, saves and closes the tab; 'browser' closes the tab.
    [ValidateSet('none', 'notepad', 'browser')][string]$Close = 'none'
)

Add-Type -TypeDefinition @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class Sim {
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk; public ushort wScan; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx; public int dy; public uint mouseData; public uint dwFlags; public uint time; public IntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct U { [FieldOffset(0)] public KEYBDINPUT ki; [FieldOffset(0)] public MOUSEINPUT mi; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public U u; }
    [DllImport("user32.dll")] static extern uint SendInput(uint n, INPUT[] inputs, int size);
    [DllImport("user32.dll")] static extern uint MapVirtualKey(uint code, uint type);
    [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll", CharSet = CharSet.Unicode)] static extern int GetWindowText(IntPtr h, StringBuilder s, int max);

    public static string Foreground() {
        var sb = new StringBuilder(512);
        GetWindowText(GetForegroundWindow(), sb, 512);
        return sb.ToString();
    }
    static INPUT Key(ushort vk, bool up) {
        var i = new INPUT(); i.type = 1;
        i.u.ki.wVk = vk; i.u.ki.wScan = (ushort)MapVirtualKey(vk, 0);
        i.u.ki.dwFlags = up ? 2u : 0u;
        return i;
    }
    public static void Down(ushort vk) { SendInput(1, new[] { Key(vk, false) }, Marshal.SizeOf(typeof(INPUT))); }
    public static void Up(ushort vk) { SendInput(1, new[] { Key(vk, true) }, Marshal.SizeOf(typeof(INPUT))); }
}
'@

function Assert-Target {
    $fg = [Sim]::Foreground()
    if ($fg -notlike "*$Title*") { throw "Foreground is '$fg', expected '*$Title*'. Stopped sending keys." }
}
function Tap([int]$vk, [int]$delay = 25) {
    Assert-Target
    [Sim]::Down($vk); Start-Sleep -Milliseconds 8; [Sim]::Up($vk); Start-Sleep -Milliseconds $delay
}
function Chord([int]$mod, [int]$vk) {
    Assert-Target
    [Sim]::Down($mod); [Sim]::Down($vk); [Sim]::Up($vk); [Sim]::Up($mod); Start-Sleep -Milliseconds 120
}
function DoubleTapConvert { Tap $TriggerVk 60; Tap $TriggerVk 0 }
function Read-Field {
    Chord 0x11 0x41   # Ctrl+A
    Chord 0x11 0x43   # Ctrl+C
    Start-Sleep -Milliseconds 150
    $text = Get-Clipboard -Raw
    Tap 0x23          # End: drop the selection
    $text
}

$saved = Get-Clipboard -Raw
try {
    Tap 0x16 150      # VK_IME_ON
    foreach ($c in $Keys.ToCharArray()) {
        $vk = switch -Regex ($c) {
            '[a-z]' { [int][char]::ToUpper($c); break }
            '[0-9]' { [int]$c; break }
            '-'     { 0xBD; break }
            ','     { 0xBC; break }
            '\.'    { 0xBE; break }
            default { throw "Unsupported key '$c'" }
        }
        Tap $vk
    }
    Start-Sleep -Milliseconds 200
    DoubleTapConvert
    Start-Sleep -Milliseconds $WaitMs
    "after polish : [$(Read-Field)]"
    if ($Undo) {
        # Read-Field typed keys, which disarms undo by design. Polish again, then undo untouched.
        Chord 0x11 0x41; Tap 0x2E   # clear the field
        foreach ($c in 'tesuto'.ToCharArray()) { Tap ([int][char]::ToUpper($c)) }
        Start-Sleep -Milliseconds 200
        DoubleTapConvert
        Start-Sleep -Milliseconds $WaitMs
        DoubleTapConvert
        Start-Sleep -Milliseconds 1200
        "after undo   : [$(Read-Field)]"
    }
    if ($Close -eq 'notepad') { Chord 0x11 0x41; Tap 0x2E; Chord 0x11 0x53; Start-Sleep -Milliseconds 300; Chord 0x11 0x57 }
    if ($Close -eq 'browser') { Chord 0x11 0x57 }
}
finally {
    if ($null -ne $saved) { Set-Clipboard -Value $saved }
}
