# Explicit metadata for operator-owned verification fixtures only. Production
# decisions are made by the Analyst model, not by this test helper.
function New-FixtureReuse($Document) {
    $fields=$Document | ConvertTo-Json -Depth 16 | ConvertFrom-Json -AsHashtable
    $anchor = if ($fields.ContainsKey('content') -and $fields.content) { [string]$fields.content } else { [string]$fields.sources[0].uri }
    @{
        applicable_when='An independent owned verification Task needs this fixture specification'
        decision_or_action='Use the pinned fixture specification to choose and verify the expected synthetic action'
        evidence=$anchor.Substring(0,[Math]::Min(160,$anchor.Length))
    }
}
