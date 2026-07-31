[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Baseline,

    [Parameter(Mandatory = $true)]
    [string] $Current,

    [ValidateRange(0, 100)]
    [double] $MaxRegressionPercent = 5.0,

    [string] $Output,

    [switch] $FailOnRegression
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Read-Summary([string] $Path) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Summary file not found: $Path"
    }

    try {
        # ConvertFrom-Json has no -Depth parameter on Windows PowerShell 5.1;
        # summaries intentionally remain shallow enough for its default parser.
        return Get-Content -LiteralPath $Path -Raw | ConvertFrom-Json
    }
    catch {
        throw "Could not parse summary '$Path': $($_.Exception.Message)"
    }
}

function Get-PropertyValue($Object, [string] $Name) {
    if ($null -eq $Object) { return $null }
    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) { return $null }
    return $property.Value
}

function Get-Direction([string] $Path, $MetricObject) {
    $declared = Get-PropertyValue $MetricObject 'direction'
    if ($declared -in @('higher', 'lower')) { return $declared }

    # Direct numeric leaves are intentionally conservative.  Add an explicit
    # direction in a run file whenever a metric is not obvious from its name.
    if ($Path -match '(?i)(fps|throughput|success(rate)?|availability)') {
        return 'higher'
    }

    return 'lower'
}

function Add-MetricLeaves($Object, [string] $Prefix, [System.Collections.Generic.List[object]] $Result) {
    if ($null -eq $Object) { return }

    if ($Object -is [System.Collections.IEnumerable] -and
        -not ($Object -is [string]) -and
        -not ($Object -is [System.Management.Automation.PSCustomObject])) {
        $index = 0
        foreach ($item in $Object) {
            Add-MetricLeaves $item "$Prefix[$index]" $Result
            $index++
        }
        return
    }

    if ($Object -is [System.Management.Automation.PSCustomObject]) {
        $valueProperty = $Object.PSObject.Properties['value']
        if ($null -ne $valueProperty -and $valueProperty.Value -is [ValueType] -and
            $valueProperty.Value -isnot [bool]) {
            $number = [double] $valueProperty.Value
            if (-not [double]::IsNaN($number) -and -not [double]::IsInfinity($number)) {
                $Result.Add([pscustomobject]@{
                    Path = $Prefix
                    Value = $number
                    Direction = Get-Direction $Prefix $Object
                })
            }
            return
        }

        foreach ($property in $Object.PSObject.Properties) {
            if ($property.Name -eq 'metadata' -or $property.Name -eq 'direction') { continue }
            $childPrefix = if ([string]::IsNullOrWhiteSpace($Prefix)) {
                $property.Name
            } else {
                "$Prefix.$($property.Name)"
            }
            Add-MetricLeaves $property.Value $childPrefix $Result
        }
        return
    }

    if ($Object -is [ValueType] -and $Object -isnot [bool]) {
        $number = [double] $Object
        if (-not [double]::IsNaN($number) -and -not [double]::IsInfinity($number)) {
            $Result.Add([pscustomobject]@{
                Path = $Prefix
                Value = $number
                Direction = Get-Direction $Prefix $null
            })
        }
    }
}

$baselineObject = Read-Summary $Baseline
$currentObject = Read-Summary $Current
$baselineMetrics = [System.Collections.Generic.List[object]]::new()
$currentMetrics = [System.Collections.Generic.List[object]]::new()
Add-MetricLeaves $baselineObject '' $baselineMetrics
Add-MetricLeaves $currentObject '' $currentMetrics

$baselineByPath = @{}
foreach ($metric in $baselineMetrics) { $baselineByPath[$metric.Path] = $metric }
$currentByPath = @{}
foreach ($metric in $currentMetrics) { $currentByPath[$metric.Path] = $metric }

$rows = [System.Collections.Generic.List[object]]::new()
foreach ($path in ($baselineByPath.Keys | Where-Object { $currentByPath.ContainsKey($_) } | Sort-Object)) {
    $baselineMetric = $baselineByPath[$path]
    $currentMetric = $currentByPath[$path]
    $direction = if ($currentMetric.Direction -in @('higher', 'lower')) {
        $currentMetric.Direction
    } else {
        $baselineMetric.Direction
    }
    $delta = $currentMetric.Value - $baselineMetric.Value
    $percent = if ($baselineMetric.Value -eq 0) {
        if ($currentMetric.Value -eq 0) { 0.0 } else { [double]::PositiveInfinity }
    } else {
        ($delta / [math]::Abs($baselineMetric.Value)) * 100.0
    }
    $regression = if ($direction -eq 'higher') {
        $percent -lt (-1.0 * $MaxRegressionPercent)
    } else {
        $percent -gt $MaxRegressionPercent
    }

    $rows.Add([pscustomobject]@{
        Metric = $path
        Direction = $direction
        Baseline = [math]::Round($baselineMetric.Value, 4)
        Current = [math]::Round($currentMetric.Value, 4)
        Delta = [math]::Round($delta, 4)
        DeltaPercent = if ([double]::IsInfinity($percent)) { $percent } else { [math]::Round($percent, 2) }
        Regression = [bool] $regression
    })
}

$missingFromCurrent = @($baselineByPath.Keys | Where-Object { -not $currentByPath.ContainsKey($_) } | Sort-Object)
$newInCurrent = @($currentByPath.Keys | Where-Object { -not $baselineByPath.ContainsKey($_) } | Sort-Object)

if ($rows.Count -gt 0) {
    $rows | Format-Table Metric, Direction, Baseline, Current, Delta, DeltaPercent, Regression -AutoSize | Out-Host
} else {
    Write-Warning 'No matching numeric metrics were found in the two summaries.'
}
if ($missingFromCurrent.Count -gt 0) { Write-Warning "Missing from current: $($missingFromCurrent -join ', ')" }
if ($newInCurrent.Count -gt 0) { Write-Host "New in current: $($newInCurrent -join ', ')" }

$report = [pscustomobject]@{
    baseline = (Resolve-Path -LiteralPath $Baseline).Path
    current = (Resolve-Path -LiteralPath $Current).Path
    maxRegressionPercent = $MaxRegressionPercent
    metrics = @($rows)
    missingFromCurrent = @($missingFromCurrent)
    newInCurrent = @($newInCurrent)
}
if (-not [string]::IsNullOrWhiteSpace($Output)) {
    $report | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $Output -Encoding utf8
    Write-Host "Wrote comparison report: $Output"
}

if ($FailOnRegression -and @($rows | Where-Object { $_.Regression }).Count -gt 0) {
    exit 2
}
